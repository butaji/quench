//! The call protocol as staged rows: the steps between a call instruction and its callee's first
//! record, and the step that takes the callee's completion back, written once for every
//! projection. The interpreter runs them on machine words (`Concrete`); the u-jit emits them as
//! IR (`Emit`) where a lane body calls a callee through its entry cell. Each row is a guard or a
//! pure word computation on what the tier loaded, so a tier keeps only where the words live:
//!
//! - admission: the callee's identity (`guard_callee`) and the room both stacks keep
//!   (`call::admit_room`, the one compare every admission is made of);
//! - `this` binding: strict takes the receiver as it is, sloppy binds an object itself and
//!   `undefined` or `null` to the global object (`bind_this`);
//! - the window: parameter `k` lives in register `k`, the rest start undefined (`fill_window`);
//! - completion: the callee's value, or a construction's receiver, once it returned
//!   (`complete_call`), every other status leaving at the caller's miss edge.
//!
//! The record a call saves and its return reads is the frame (`staged::frame`, its header words
//! and the lane's activation record); the receiver rule of a
//! construction through an initializer is `construct`.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{InvocationFormsExt};
use super::call::admit_room;
use super::frame;
use super::layout_offsets::{JS_STEP_BODY_TOP, JS_STEP_LANE_VALUES_TOP, JS_STEP_PLAIN_DEPTH, JS_STEP_RETURNED_PC};
use super::region::Region;
use super::value::{is_direct_array, is_direct_function, is_direct_object};
use super::{word_bytes, Invocation, Passed, Stage, Step};
use crate::bytecode::Register;
use crate::value::ValueLayout;
use crate::Value;

pub use super::native_frame::{pop_native_frame, push_native_frame, set_body_top, set_values, LaneWords, NativeFrame, NativePlaces, PayloadTops};

const _: () = assert!(std::mem::size_of::<Value>() == std::mem::size_of::<u64>(), "a register of a window is one word");

/// The callee's window of a plain call: `registers` words, of which the first `argc` are the
/// arguments at register `first` of the caller's window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct WindowFill {
    pub(crate) first: Register,
    pub(crate) argc: u16,
    pub(crate) registers: u16,
}

/// A call-protocol step `step` runs, with the words a tier loaded. Each yields the word it
/// checked or produced; a failed guard leaves at the frame's miss edge.
#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))] // the guards and the window are rows for the form-generic lane helpers
pub enum CallOperation<W> {
    /// The operands of a call instruction, which no row runs alone (`Invocation::of`).
    Site(Invocation),
    /// Continue where `held` is the callee identity `expected` (a per-site operand).
    Callee { held: W, expected: u64 },
    /// The `this` of the callee for `receiver`: `sloppy` is the global object a sloppy callee
    /// binds for `undefined` and `null`, `None` for a strict callee.
    BindThis { receiver: W, sloppy: Option<u64> },
    /// Continue where the construction plan allocated `receiver` (not the deleted marker).
    Receiver { receiver: W },
    /// Continue where `needed` fits within `limit`.
    Room { limit: W, needed: W },
    /// The callee's window at `top`, filled from the caller's registers at `source`.
    Window { top: W, source: W, fill: WindowFill },
    /// The call's value once `status` is `returned`: the construction's `receiver` where there is
    /// one, else the callee's `value`.
    Complete { status: W, value: W, receiver: Option<W>, returned: u32 },
}

/// Run the call-protocol `operation`; its word, or `Unresolved` where it has no row of its own.
#[inline(always)]
pub(crate) fn step<S: Stage, C>(stage: &mut S, miss: S::Miss, operation: CallOperation<S::Word>) -> Step<S::Word, C> {
    let word = match operation {
        CallOperation::Site(_) => None,
        CallOperation::Callee { held, expected } => guard_callee(stage, held, expected, miss).map(|()| held),
        CallOperation::Receiver { receiver } => guard_received(stage, receiver, miss).map(|()| receiver),
        CallOperation::BindThis { receiver, sloppy } => bind_this(stage, receiver, sloppy, miss),
        CallOperation::Room { limit, needed } => admit_room(stage, limit, needed, miss).map(|()| needed),
        CallOperation::Window { top, source, fill } => {
            fill_window(stage, top, source, fill);
            Some(top)
        }
        CallOperation::Complete { status, value, receiver, returned } =>
            complete_call(stage, status, value, receiver, returned, miss),
    };
    word.map_or(Step::Unresolved, Step::Value)
}

/// Continues where the callee register holds the identity the site was bound to.
#[inline(always)]
pub(crate) fn guard_callee<S: Stage>(stage: &mut S, held: S::Word, expected: u64, miss: S::Miss) -> Option<()> {
    let same = stage.eq_imm(held, expected);
    stage.guard(same, miss)
}

/// Continues where a construction's receiver was allocated: the plan answers the deleted marker
/// where it declines (a due collection, a plan that does not hold).
#[inline(always)]
pub fn guard_received<S: Stage>(stage: &mut S, receiver: S::Word, miss: S::Miss) -> Option<()> {
    let built = stage.ne_imm(receiver, Value::DELETED.to_bits());
    stage.guard(built, miss)
}

/// Whether `bits` names an object of any direct family (`Value::is_direct_object_family`): an
/// ordinary object, array, function, promise, iterator or collection. `object` is the test of the
/// first, which the caller made.
#[inline(always)]
fn is_object_family<S: Stage>(stage: &mut S, bits: S::Word, object: S::Cond) -> S::Cond {
    let array = is_direct_array(stage, bits);
    let function = is_direct_function(stage, bits);
    let family = stage.and_imm(bits, ValueLayout::HEAP_FAMILY_MASK);
    let promise = stage.eq_imm(family, ValueLayout::DIRECT_PROMISE_TAG);
    let iterator = stage.eq_imm(family, ValueLayout::DIRECT_ITERATOR_TAG);
    let collection = stage.eq_imm(family, ValueLayout::DIRECT_COLLECTION_TAG);
    let records = stage.any(object, array);
    let others = stage.any(function, promise);
    let objects = stage.any(records, others);
    let iterators = stage.any(objects, iterator);
    stage.any(iterators, collection)
}

/// The `this` a call binds for `receiver`. A strict callee takes any receiver as it is. A sloppy
/// callee binds an object receiver itself and `undefined` or `null` to `global`; any other
/// receiver (a primitive to box) leaves at `miss`, the host's to bind.
#[inline(always)]
pub(crate) fn bind_this<S: Stage>(stage: &mut S, receiver: S::Word, sloppy: Option<u64>, miss: S::Miss)
    -> Option<S::Word> {
    bind_this_with(stage, receiver, sloppy.map(|global| move |stage: &mut S| stage.constant(global)), miss)
}

/// `bind_this` with the global a sloppy callee binds made by `global` where the binding needs it: a
/// constant, or a word the frame reads (`CallFrame::call_global`).
#[inline(always)]
pub(crate) fn bind_this_with<S: Stage, G: FnOnce(&mut S) -> S::Word>(stage: &mut S, receiver: S::Word, global: Option<G>, miss: S::Miss)
    -> Option<S::Word> {
    let Some(global) = global else { return Some(receiver) };
    let ordinary = is_direct_object(stage, receiver);
    // A stage that knows its words (the interpreter) binds the common ordinary object on that one test.
    if stage.known(ordinary) == Some(true) { return Some(receiver); }
    let object = is_object_family(stage, receiver, ordinary);
    let undefined = stage.eq_imm(receiver, Value::UNDEFINED.to_bits());
    let null = stage.eq_imm(receiver, Value::NULL.to_bits());
    let nullish = stage.any(undefined, null);
    let bound = stage.any(object, nullish);
    stage.guard(bound, miss)?;
    let global = global(stage);
    Some(stage.select(object, receiver, global))
}

/// `bind_this_with` where the callee's sloppiness is a condition of the stage (`sloppy`, true where the callee is sloppy):
/// a strict callee's receiver is its own, a sloppy one's binds as `bind_this_with` binds it.
pub(crate) fn bind_this_dynamic<S: Stage, G: FnOnce(&mut S) -> S::Word>(stage: &mut S, receiver: S::Word, sloppy: S::Cond,
    global: G, miss: S::Miss) -> Option<S::Word> {
    let ordinary = is_direct_object(stage, receiver);
    let object = is_object_family(stage, receiver, ordinary);
    let undefined = stage.eq_imm(receiver, Value::UNDEFINED.to_bits());
    let null = stage.eq_imm(receiver, Value::NULL.to_bits());
    let nullish = stage.any(undefined, null);
    let bound = stage.any(object, nullish);
    let strict = stage.not(sloppy);
    let fine = stage.any(strict, bound);
    stage.guard(fine, miss)?;
    let global = global(stage);
    let bound_this = stage.select(object, receiver, global);
    Some(stage.select(sloppy, bound_this, receiver))
}

/// `bind_this` on machine words for the interpreter's own binding of a sloppy callee's `this`:
/// the same row the native tiers emit.
#[inline(always)]
pub fn concrete_bind_this(receiver: u64, global: u64) -> Option<u64> {
    bind_this(&mut super::Concrete::pure(), receiver, Some(global), ())
}

/// The callee's window at `top`: parameter `k` lives in register `k` of a plain activation, so
/// the `argc` arguments at `source` fill its first registers and the rest start undefined.
#[inline(always)]
pub(crate) fn fill_window<S: Stage>(stage: &mut S, top: S::Word, source: S::Word, fill: WindowFill) {
    let arguments = stage.add_imm(source, word_bytes(fill.first));
    fill_call(stage, top, &[], Passed::Window { arguments, count: fill.argc }, fill.registers);
}

/// The callee's window at `top` of any call form: the `prefix` words a bound function binds
/// (constants of the plan), then the `passed` arguments, the rest undefined. One row for the
/// register window, the register list and the bound prefix.
#[inline(always)]
pub fn fill_call<S: Stage>(stage: &mut S, top: S::Word, prefix: &[u64], passed: Passed<'_, S::Word>, registers: u16) {
    let undefined = stage.constant(Value::UNDEFINED.to_bits());
    stage.fill_window(top, undefined, prefix, passed, registers);
}

/// The value of a call whose callee ended with `status`: the construction's `receiver` where
/// there is one (a forwarding constructor yields its receiver whatever the initializer
/// returned), else the callee's `value`. Any status but `returned` leaves at `miss`, where the
/// tier tells a callee that never entered from one that left.
#[inline(always)]
pub(crate) fn complete_call<S: Stage>(stage: &mut S, status: S::Word, value: S::Word, receiver: Option<S::Word>,
    returned: u32, miss: S::Miss) -> Option<S::Word> {
    let status = stage.uextend32(status);
    let done = stage.eq_imm(status, u64::from(returned));
    stage.guard_status(done, miss)?;
    Some(receiver.unwrap_or(value))
}

/// A plain activation's frame header (call protocol v2 stage C), the context words below its
/// window at `frame::CONTEXT_CALL`..`CONTEXT_CALLEE`, written in place of an activation record.
#[derive(Clone, Copy)]
pub struct PlainHeader<W> {
    /// The caller's call record.
    pub call: W,
    pub(crate) caller_registers: W,
    /// The caller's records (the lane context's `base`).
    pub caller_base: W,
    /// `plain_packed`'s word.
    pub(crate) packed: W,
    pub(crate) callee: W,
}

impl PlainHeader<u64> {
    /// The header of a plain activation of `function` with `argc` values and side-record `flags`,
    /// called from `call` with its caller's `caller_registers` and `caller_base`, running
    /// `callee`. The one constructor of the words every plain push writes: the interpreter's
    /// record-shaped push (`LaneStack::push_activation`), a proved call (`enter_plain_window`)
    /// and a native callback's trampoline header.
    #[inline(always)]
    pub const fn new(call: u64, caller_registers: u64, caller_base: u64, function: u32, argc: u16, flags: u64,
        callee: u64) -> Self {
        Self { call, caller_registers, caller_base, packed: plain_packed(function, argc, flags), callee }
    }
}

/// What a plain activation's return reads of its header: the caller's call record, registers
/// and records.
#[derive(Clone, Copy)]
pub struct PlainCaller<W> {
    pub call: W,
    pub registers: W,
    pub base: W,
}

/// The header's packed word: `function`, the plain flag and `flags` (`frame::BODY_FLAG`,
/// `frame::NATIVE_FLAG`), and `argc`.
#[inline(always)]
pub(crate) const fn plain_packed(function: u32, argc: u16, flags: u64) -> u64 {
    function as u64 | frame::PLAIN_FLAG | flags | ((argc as u64) << frame::ARGC_SHIFT)
}

/// The byte offset of context word `word` below a window's register 0.
#[inline(always)]
const fn context_at(word: usize) -> i32 { -((word * 8) as i32) }

/// Push a plain activation whose window starts at `window`: its header below the window, then
/// the plain activations' words at `plain` (`frame::PLAIN_*_AT`) count it and name its window
/// and program. One row for the interpreter, the b-jit's direct calls and the u-jit's. The
/// tier moves the values' top past the window itself.
#[inline(always)]
pub fn push_plain<S: Stage>(stage: &mut S, plain: S::Word, window: S::Word, header: PlainHeader<S::Word>, program: S::Word) {
    let depth = stage.load_u64(Region::Payload.mutable(), plain, frame::PLAIN_DEPTH_AT as i32);
    push_plain_at(stage, plain, window, header, program, depth);
}

/// `push_plain` where the tier already holds the plain depth word `depth` (it loaded it for its
/// admission, `LaneStack::plain_room`): the same stores, without reloading it.
#[inline(always)]
pub fn push_plain_at<S: Stage>(stage: &mut S, plain: S::Word, window: S::Word, header: PlainHeader<S::Word>, program: S::Word, depth: S::Word) {
    let at = Region::Frame.mutable();
    stage.store_u64(at, window, context_at(frame::CONTEXT_CALL), header.call);
    stage.store_u64(at, window, context_at(frame::CONTEXT_CALLER_REGISTERS), header.caller_registers);
    stage.store_u64(at, window, context_at(frame::CONTEXT_CALLER_BASE), header.caller_base);
    stage.store_u64(at, window, context_at(frame::CONTEXT_PACKED), header.packed);
    stage.store_u64(at, window, context_at(frame::CONTEXT_CALLEE), header.callee);
    let words = Region::Payload.mutable();
    let depth = stage.add_imm(depth, 1u64);
    stage.store_u64(words, plain, frame::PLAIN_DEPTH_AT as i32, depth);
    stage.store_u64(words, plain, frame::PLAIN_REGISTERS_AT as i32, window);
    stage.store_u32(words, plain, frame::PLAIN_PROGRAM_AT as i32, program);
}

/// Pop the innermost plain activation, whose window is `window`: the plain activations' words
/// at `plain` drop it and name its caller's window, which is the next plain activation's where
/// one remains. Its caller, which the return resumes. The tier releases the window itself.
#[inline(always)]
pub fn pop_plain<S: Stage>(stage: &mut S, plain: S::Word, window: S::Word) -> PlainCaller<S::Word> {
    let at = Region::Frame.mutable();
    let call = stage.load_u64(at, window, context_at(frame::CONTEXT_CALL));
    let registers = stage.load_u64(at, window, context_at(frame::CONTEXT_CALLER_REGISTERS));
    let base = stage.load_u64(at, window, context_at(frame::CONTEXT_CALLER_BASE));
    let words = Region::Payload.mutable();
    let depth = stage.load_u64(words, plain, frame::PLAIN_DEPTH_AT as i32);
    let depth = stage.add_imm(depth, u64::MAX);
    stage.store_u64(words, plain, frame::PLAIN_DEPTH_AT as i32, depth);
    stage.store_u64(words, plain, frame::PLAIN_REGISTERS_AT as i32, registers);
    PlainCaller { call, registers, base }
}

/// The addresses the lane's running state lives at, from the dispatch payload: the values' top word,
/// the plain-frame depth word (its registers word is `frame::PLAIN_REGISTERS_AT` past it), and the body
/// table's top word. Read once where a call is admitted; every move writes through them.
#[derive(Clone, Copy)]
pub struct LaneTops<W> {
    pub values_at: W,
    pub plain_at: W,
    pub body_top_at: W,
}

/// The lane's state addresses, from the dispatch payload.
#[allow(dead_code)] // the b-jit stencils' call rows
#[inline(always)]
pub fn lane_tops<S: Stage>(stage: &mut S, payload: S::Word) -> LaneTops<S::Word> {
    let values_at = stage.load_u64(Region::Dispatch.mutable(), payload, JS_STEP_LANE_VALUES_TOP as i32);
    let plain_at = stage.load_u64(Region::Dispatch.mutable(), payload, JS_STEP_PLAIN_DEPTH as i32);
    let body_top_at = stage.load_u64(Region::Dispatch.mutable(), payload, JS_STEP_BODY_TOP as i32);
    LaneTops { values_at, plain_at, body_top_at }
}

/// Moves the plain frames to `depth`, with the innermost window at `registers` (`PlainFrames`).
#[allow(dead_code)] // the b-jit stencils' call rows
#[inline(always)]
pub fn set_plain<S: Stage>(stage: &mut S, plain_at: S::Word, depth: S::Word, registers: S::Word) {
    stage.store_u64(Region::Payload.mutable(), plain_at, super::frame::PLAIN_DEPTH_AT as i32, depth);
    stage.store_u64(Region::Payload.mutable(), plain_at, super::frame::PLAIN_REGISTERS_AT as i32, registers);
}

/// The header words a body frame writes below its window (the call protocol v2 context block).
#[derive(Clone, Copy)]
pub struct BodyHeader<W> {
    pub call: W,
    pub caller_registers: W,
    pub caller_base: W,
    pub packed: W,
    pub callee: W,
}

/// Writes a body frame's header at `window`: each word at its `frame::CONTEXT_*` index counted down from
/// register 0. The receiver and captures words are the caller's to write (`CONTEXT_THIS`, `CONTEXT_BODY`).
#[allow(dead_code)] // the b-jit stencils' call rows
#[inline(always)]
pub fn push_body_header<S: Stage>(stage: &mut S, window: S::Word, header: BodyHeader<S::Word>) {
    use super::frame::{CONTEXT_CALL, CONTEXT_CALLEE, CONTEXT_CALLER_BASE, CONTEXT_CALLER_REGISTERS, CONTEXT_PACKED};
    let at = |index: usize| -((index * core::mem::size_of::<u64>()) as i32);
    stage.store_u64(Region::Frame.mutable(), window, at(CONTEXT_CALL), header.call);
    stage.store_u64(Region::Frame.mutable(), window, at(CONTEXT_CALLER_REGISTERS), header.caller_registers);
    stage.store_u64(Region::Frame.mutable(), window, at(CONTEXT_CALLER_BASE), header.caller_base);
    stage.store_u64(Region::Frame.mutable(), window, at(CONTEXT_PACKED), header.packed);
    stage.store_u64(Region::Frame.mutable(), window, at(CONTEXT_CALLEE), header.callee);
}

/// Records `pc`, the record a raised exception unwinds from, in the dispatch payload
/// (`JS_STEP_RETURNED_PC`) for the lane to read.
#[allow(dead_code)] // the b-jit stencils' call rows
#[inline(always)]
pub(crate) fn record_raised_pc<S: Stage>(stage: &mut S, payload: S::Word, pc: S::Word) {
    stage.store_u64(Region::Dispatch.mutable(), payload, JS_STEP_RETURNED_PC as i32, pc);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::{Arguments, CallFacts, Callee, Receiver};
    use crate::staged::call::concrete_fits;
    use crate::staged::test_words::FACTS;
    use crate::staged::Concrete;

    const RETURNED: u32 = 7;

    fn run(operation: CallOperation<u64>) -> Option<u64> {
        match step::<Concrete, ()>(&mut Concrete::pure(), (), operation) {
            Step::Value(word) => Some(word),
            _ => None,
        }
    }

    #[test]
    fn a_site_runs_no_row_alone_and_a_callee_must_be_the_bound_one() {
        let site = Invocation { callee: Callee::Register(1), receiver: Receiver::Register(2), arguments: Arguments::Window { first: 3, count: 1 },
            result: 0, returns: false, forwards: false };
        assert_eq!(run(CallOperation::Site(site)), None);
        assert_eq!(run(CallOperation::Callee { held: 9, expected: 9 }), Some(9));
        assert_eq!(run(CallOperation::Callee { held: 8, expected: 9 }), None);
        assert_eq!(run(CallOperation::Receiver { receiver: 3 }), Some(3));
        assert_eq!(run(CallOperation::Receiver { receiver: Value::DELETED.to_bits() }), None, "a declined plan leaves");
    }

    #[test]
    fn admission_is_one_unsigned_compare_where_equal_fits() {
        assert!(concrete_fits(4, 4) && concrete_fits(5, 4) && !concrete_fits(3, 4));
        assert_eq!(run(CallOperation::Room { limit: 10, needed: 10 }), Some(10));
        assert_eq!(run(CallOperation::Room { limit: 9, needed: 10 }), None);
        assert!(!concrete_fits(0, u64::MAX), "an unsigned compare, not a signed one");
    }

    #[test]
    fn a_strict_callee_takes_any_receiver_and_a_sloppy_one_binds_by_the_value() {
        let mut heap = crate::heap::Heap::new();
        let object = heap.alloc_object(crate::heap::Object::new(Value::NULL));
        let text = heap.alloc_string("x".into());
        let global = Value::integer(99).to_bits();
        let bind = |receiver: Value, sloppy: Option<u64>| run(CallOperation::BindThis { receiver: receiver.to_bits(), sloppy });
        assert_eq!(bind(Value::integer(5), None), Some(Value::integer(5).to_bits()));
        assert_eq!(bind(object, Some(global)), Some(object.to_bits()));
        assert_eq!(bind(Value::UNDEFINED, Some(global)), Some(global));
        assert_eq!(bind(Value::NULL, Some(global)), Some(global));
        assert_eq!(bind(Value::integer(5), Some(global)), None, "a primitive is boxed by the host");
        assert_eq!(bind(text, Some(global)), None, "a string is boxed by the host");
    }

    /// The row's family test is the value's own (`Value::is_direct_object_family`).
    #[test]
    fn the_object_family_is_the_values_own() {
        let mut heap = crate::heap::Heap::new();
        let object = heap.alloc_object(crate::heap::Object::new(Value::NULL));
        let text = heap.alloc_string("x".into());
        let mut stage = Concrete::pure();
        for value in [object, text, Value::UNDEFINED, Value::NULL, Value::integer(1), Value::number(1.5), Value::TRUE] {
            let ordinary = is_direct_object(&mut stage, value.to_bits());
            assert_eq!(is_object_family(&mut stage, value.to_bits(), ordinary), value.is_direct_object_family(), "{value:?}");
        }
    }

    #[test]
    fn a_window_takes_the_arguments_then_undefined() {
        let source = [10_u64, 20, 30];
        let mut window = [0_u64; 4];
        let fill = WindowFill { first: 1, argc: 2, registers: 4 };
        let result = run(CallOperation::Window { top: window.as_mut_ptr() as u64, source: source.as_ptr() as u64, fill });
        assert_eq!(result, Some(window.as_ptr() as u64));
        assert_eq!(window, [20, 30, Value::UNDEFINED.to_bits(), Value::UNDEFINED.to_bits()]);
    }

    #[test]
    fn a_completion_yields_the_value_or_the_receiver_only_once_returned() {
        let complete = |status, receiver| run(CallOperation::Complete { status, value: 1, receiver, returned: RETURNED });
        assert_eq!(complete(u64::from(RETURNED), None), Some(1));
        assert_eq!(complete(u64::from(RETURNED), Some(2)), Some(2), "a construction yields its receiver");
        assert_eq!(complete(u64::from(RETURNED) + 1, None), None);
        assert_eq!(complete(u64::from(RETURNED) + 1, Some(2)), None);
    }

    /// A row's words are the tier's: the protocol has no per-site hole but the callee identity,
    /// a per-site operand the stage keeps as a constant.
    #[test]
    fn the_rows_have_no_per_site_hole() {
        let mut trace = crate::staged::trace::Trace::default();
        let words = [trace.constant(0), trace.constant(1), trace.constant(2)];
        guard_callee(&mut trace, words[0], 5, ());
        bind_this(&mut trace, words[0], Some(6), ());
        fill_window(&mut trace, words[0], words[1], WindowFill { first: 0, argc: 1, registers: 2 });
        complete_call(&mut trace, words[0], words[1], Some(words[2]), RETURNED, ());
        assert!(trace.holes.is_empty());
    }

    #[test]
    fn a_return_is_admitted_by_its_register_and_the_tier_completing() {
        let row = crate::staged::Operation::<u64, ()>::Return { source: 9 };
        assert_eq!(row.admitted(FACTS), Some(true));
        assert_eq!(crate::staged::Operation::<u64, ()>::Return { source: 10 }.admitted(FACTS), Some(false));
        assert_eq!(row.admitted(CallFacts { activates: false, ..FACTS }), Some(false));
        assert_eq!(crate::staged::Operation::<u64, ()>::Jump.admitted(FACTS), None);
    }
}

#[cfg(not(stencil_build))]
/// Where a call site goes on a status its call helper returns (`LaneHelper::Call`,
/// `LaneHelper::ConstructCall`): completed with its result written, missed before any effect,
/// or raised with the exception stashed. Any other status leaves the region with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HelperOutcome {
    Completed,
    Missed,
    Raised,
}

#[cfg(not(stencil_build))]
impl HelperOutcome {
    /// The exit this outcome is (`exit::Exit`, the one meaning of a status).
    pub const fn exit(self) -> super::exit::Exit {
        match self {
            Self::Completed => super::exit::Exit::Completed,
            Self::Missed => super::exit::Exit::Missed,
            Self::Raised => super::exit::Exit::Raised,
        }
    }

    /// The helper status word the outcome is encoded as: the helper encoding of its exit.
    pub const fn word(self) -> u64 {
        match self.exit().to_helper() {
            Some(word) => word,
            None => unreachable!(),
        }
    }
}

#[cfg(not(stencil_build))]
/// The helper statuses a call site tests, in the order it tests them (the common one first),
/// paired with where each goes: derived from each outcome's exit, the one table both the helper and
/// its call sites share.
pub const HELPER_OUTCOMES: [(u64, HelperOutcome); 3] = [
    (HelperOutcome::Completed.word(), HelperOutcome::Completed),
    (HelperOutcome::Missed.word(), HelperOutcome::Missed),
    (HelperOutcome::Raised.word(), HelperOutcome::Raised),
];
