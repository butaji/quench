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
//! The record a call saves and its return reads is `call::Record`; the receiver rule of a
//! construction through an initializer is `construct`.

use super::call::admit_room;
use super::value::{is_direct_array, is_direct_function, is_direct_object};
use super::{Stage, Step};
use crate::bytecode::{ConstructArguments, ImmediateLayout, Op, Register, WideInstruction};
use crate::value::ValueLayout;
use crate::Value;

/// Bytes in one register of a window.
const VALUE_BYTES: i32 = std::mem::size_of::<Value>() as i32;

/// The static operands of a call instruction: where its callee, receiver and arguments live and
/// where its result goes. A construction has no receiver register: the plan allocates it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Invocation {
    pub(crate) callee: Register,
    pub(crate) receiver: Option<Register>,
    pub(crate) window: Register,
    pub(crate) argc: u16,
    pub(crate) result: Register,
}

impl Invocation {
    /// The operands of `instruction`, a call through a register window or a construction with its
    /// arguments in registers; `None` for any other call form (spread, array, method), whose
    /// window is not in registers.
    pub(crate) fn of(instruction: WideInstruction) -> Option<Self> {
        let imm = instruction.imm();
        let (receiver, window, argc) = match instruction.op() {
            Op::Call | Op::CallForwardingArguments => (Some(instruction.register_c()),
                ImmediateLayout::call_window_base(imm), ImmediateLayout::argument_count(imm)),
            Op::Construct => match instruction.construct_arguments() {
                ConstructArguments::Registers(window) => (None, window.base, window.count),
                ConstructArguments::Array(_) => return None,
            },
            _ => return None,
        };
        Some(Self { callee: instruction.register_b(), receiver, window, argc, result: instruction.result_register() })
    }
}

/// The callee's window: `registers` words, of which the first `argc` are the arguments at
/// register `first` of the caller's window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct WindowFill {
    pub(crate) first: Register,
    pub(crate) argc: u16,
    pub(crate) registers: u16,
}

/// A call-protocol step `step` runs, with the words a tier loaded. Each yields the word it
/// checked or produced; a failed guard leaves at the frame's miss edge.
#[derive(Clone, Copy)]
pub(crate) enum CallOperation<W> {
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
pub(crate) fn guard_received<S: Stage>(stage: &mut S, receiver: S::Word, miss: S::Miss) -> Option<()> {
    let built = stage.ne_imm(receiver, Value::DELETED.to_bits());
    stage.guard(built, miss)
}

/// Whether `bits` names an object of any direct family (`Value::is_direct_object_family`): an
/// ordinary object, array, function, promise or iterator record.
#[inline(always)]
fn is_object_family<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let object = is_direct_object(stage, bits);
    let array = is_direct_array(stage, bits);
    let function = is_direct_function(stage, bits);
    let family = stage.and_imm(bits, ValueLayout::HEAP_FAMILY_MASK);
    let promise = stage.eq_imm(family, ValueLayout::DIRECT_PROMISE_TAG);
    let iterator = stage.eq_imm(family, ValueLayout::DIRECT_ITERATOR_TAG);
    let records = stage.any(object, array);
    let others = stage.any(function, promise);
    let objects = stage.any(records, others);
    stage.any(objects, iterator)
}

/// The `this` a call binds for `receiver`. A strict callee takes any receiver as it is. A sloppy
/// callee binds an object receiver itself and `undefined` or `null` to `global`; any other
/// receiver (a primitive to box) leaves at `miss`, the host's to bind.
#[inline(always)]
pub(crate) fn bind_this<S: Stage>(stage: &mut S, receiver: S::Word, sloppy: Option<u64>, miss: S::Miss)
    -> Option<S::Word> {
    let Some(global) = sloppy else { return Some(receiver) };
    let object = is_object_family(stage, receiver);
    let undefined = stage.eq_imm(receiver, Value::UNDEFINED.to_bits());
    let null = stage.eq_imm(receiver, Value::NULL.to_bits());
    let nullish = stage.any(undefined, null);
    let bound = stage.any(object, nullish);
    stage.guard(bound, miss)?;
    let global = stage.constant(global);
    Some(stage.select(object, receiver, global))
}

/// The byte offset of register `register` in a window.
#[inline(always)]
const fn window_offset(register: u16) -> i32 {
    register as i32 * VALUE_BYTES
}

/// The callee's window at `top`: parameter `k` lives in register `k` of a plain activation, so
/// the `argc` arguments at `source` fill its first registers and the rest start undefined.
#[inline(always)]
pub(crate) fn fill_window<S: Stage>(stage: &mut S, top: S::Word, source: S::Word, fill: WindowFill) {
    let undefined = stage.constant(Value::UNDEFINED.to_bits());
    for register in 0..fill.registers {
        let value = if register < fill.argc {
            stage.load_u64(source, window_offset(fill.first + register))
        } else {
            undefined
        };
        stage.store_u64(top, window_offset(register), value);
    }
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
    stage.guard(done, miss)?;
    Some(receiver.unwrap_or(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::call::concrete_fits;
    use crate::staged::Concrete;

    const RETURNED: u32 = 7;

    fn run(operation: CallOperation<u64>) -> Option<u64> {
        match step::<Concrete, ()>(&mut Concrete::new(std::ptr::null()), (), operation) {
            Step::Value(word) => Some(word),
            _ => None,
        }
    }

    #[test]
    fn a_site_runs_no_row_alone_and_a_callee_must_be_the_bound_one() {
        let site = Invocation { callee: 1, receiver: Some(2), window: 3, argc: 1, result: 0 };
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
        let object = heap.alloc(crate::heap::Cell::Object(crate::heap::Object::new(Value::NULL)));
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
        let object = heap.alloc(crate::heap::Cell::Object(crate::heap::Object::new(Value::NULL)));
        let text = heap.alloc_string("x".into());
        let mut stage = Concrete::new(std::ptr::null());
        for value in [object, text, Value::UNDEFINED, Value::NULL, Value::integer(1), Value::number(1.5), Value::TRUE] {
            assert_eq!(is_object_family(&mut stage, value.to_bits()), value.is_direct_object_family(), "{value:?}");
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

    fn call_instruction(op: Op, constructs: bool) -> WideInstruction {
        if constructs {
            WideInstruction::new(op, 4, 5, 6, ImmediateLayout::construct_immediate(2, false, false))
        } else {
            WideInstruction::new(op, 4, 5, 6, ImmediateLayout::call_immediate(7, 2, false, false))
        }
    }

    #[test]
    fn the_operands_of_a_call_and_a_construction_decode_once() {
        let call = Invocation::of(call_instruction(Op::Call, false)).expect("a call decodes");
        assert_eq!(call, Invocation { callee: 5, receiver: Some(6), window: 7, argc: 2, result: 4 });
        let construct = Invocation::of(call_instruction(Op::Construct, true)).expect("a construction decodes");
        assert_eq!((construct.callee, construct.receiver, construct.argc, construct.result), (5, None, 2, 4));
        assert_eq!(Invocation::of(call_instruction(Op::Move, false)), None);
        let decoded = crate::staged::Operation::<u64, ()>::decode(call_instruction(Op::Call, false), |_| None);
        assert!(matches!(decoded, Some(crate::staged::Operation::Call(CallOperation::Site(site))) if site == call));
    }
}
