//! The one call row (call protocol v2, C1): `enter` and its mirror `complete`, composed from the
//! family rows. Every call path names its facts in a `CallKind` and runs `enter`; a return runs
//! `complete`. Stage-generic, no alloc or std collections, so the interpreter and the tiers run
//! the same steps.
//!
//! `enter`, in order:
//!
//! 1. the callee identity (`protocol::guard_callee`), where the site is bound to one;
//! 2. admission: the plain depth after this call is within the bound, and the window fits the
//!    values' room (`call::admit_room`, twice);
//! 3. the `this` binding, where the callee binds one (`protocol::bind_this`);
//! 4. for a plain-header family: the window (`protocol::fill_window`), the context words `this`,
//!    `env`, `capture` below register 0, and the header (`protocol::push_plain_at`).
//!
//! The other families (construct, callback, iteration, keeping, environment, leaf) stop after
//! step 3. Their windows, context words and records are the family rows' (`call_construct`,
//! `call_callback`, `call_iteration`, `call_keeping`, `call_environment`, `call_leaf`), so the
//! callers of those families run their own fill and push after `enter`.
//!
//! `complete` mirrors it: the callee's outcome is guarded first (`protocol::complete_call`), then
//! the header is popped (`protocol::pop_plain`), so a refused completion changes nothing.

use super::call::admit_room;
use super::frame;
use super::protocol::{
    bind_this, complete_call, fill_window, guard_callee, plain_packed, pop_plain, push_plain_at, PlainCaller,
    PlainHeader, WindowFill,
};
use super::region::Region;
use super::Stage;
use crate::bytecode::Register;

/// The call families `enter` serves. A plain-header family (`plain_header`) gets a full plain
/// activation; the rest stop after the shared steps and run their family's own row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Family {
    Plain,
    Method,
    Closure,
    Construct,
    Callback,
    Iteration,
    Keeping,
    Environment,
    Leaf,
}

impl Family {
    /// Whether a call of this family is a plain activation: a window, context words and a header.
    pub(crate) const fn plain_header(self) -> bool {
        matches!(self, Family::Plain | Family::Method | Family::Closure)
    }
}

/// The facts of one call site or link, folded per projection: the family, the callee's packed
/// word (`function`, `argc`, `flags`), the window (`registers`, the caller's `first` argument
/// register), and the `this` rule (`needs_this`, `sloppy`'s global object). `callee` is the
/// identity the site was bound to, `None` where the identity is folded. `returned` is the status
/// a callee's return carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CallKind {
    pub(crate) family: Family,
    pub(crate) function: u32,
    pub(crate) argc: u16,
    pub(crate) registers: u16,
    pub(crate) first: Register,
    pub(crate) needs_this: bool,
    pub(crate) sloppy: Option<u64>,
    pub(crate) callee: Option<u64>,
    pub(crate) flags: u64,
    pub(crate) returned: u32,
}

impl CallKind {
    /// The header's packed word for this kind (`protocol::plain_packed`).
    #[allow(dead_code)] // C2-C5 route paths through it
    pub(crate) const fn packed(self) -> u64 {
        plain_packed(self.function, self.argc, self.flags)
    }
}

/// The words a tier loaded for one call. `held` is the callee identity word, `receiver` the
/// receiver's bits. `depth` is the plain depth word and `bound` the plain bound (both of
/// `PlainFrames`), `free` the values' room, `needed` the window's words. `plain` is the
/// `PlainFrames` address and `top` the callee's window. `registers` is the caller's register
/// base (the arguments' source), `base` the caller's records, and `call` the caller's call
/// record. `env`, `capture` and `program` are the callee's, and the caller's program.
#[derive(Clone, Copy)]
pub(crate) struct EnterWords<W> {
    pub(crate) held: W,
    pub(crate) receiver: W,
    pub(crate) env: W,
    pub(crate) capture: W,
    pub(crate) depth: W,
    pub(crate) bound: W,
    pub(crate) free: W,
    pub(crate) needed: W,
    pub(crate) plain: W,
    pub(crate) top: W,
    pub(crate) registers: W,
    pub(crate) base: W,
    pub(crate) call: W,
    pub(crate) program: W,
}

/// What `enter` yields: the callee's window and its bound `this`.
#[derive(Clone, Copy)]
pub(crate) struct Entered<W> {
    pub(crate) window: W,
    pub(crate) this: W,
}

/// The callee's outcome at a return: its `status` and `value`, and the construction's `receiver`
/// where there is one. `returned` is the status of a normal return.
#[derive(Clone, Copy)]
pub(crate) struct CallOutcome<W> {
    pub(crate) status: W,
    pub(crate) value: W,
    pub(crate) receiver: Option<W>,
    pub(crate) returned: u32,
}

/// What `complete` yields: the call's value and the caller the return resumes.
#[derive(Clone, Copy)]
pub(crate) struct Completed<W> {
    pub(crate) value: W,
    pub(crate) caller: PlainCaller<W>,
}

/// The byte offset of context word `word` below a window's register 0.
#[inline(always)]
const fn context_at(word: usize) -> i32 {
    -((word * 8) as i32)
}

/// Enter a call of `kind` with `words`: the shared steps of every call (see the module docs).
/// Leaves at `miss` where a guard, the admission or the `this` binding refuses; no word is
/// written before the last guard passes.
#[allow(dead_code)] // C2-C5 route paths through it
#[inline(always)]
pub(crate) fn enter<S: Stage>(stage: &mut S, kind: CallKind, words: EnterWords<S::Word>, miss: S::Miss)
    -> Option<Entered<S::Word>> {
    if let Some(expected) = kind.callee {
        guard_callee(stage, words.held, expected, miss)?;
    }
    let depth = stage.add_imm(words.depth, 1u64);
    admit_room(stage, words.bound, depth, miss)?;
    admit_room(stage, words.free, words.needed, miss)?;
    let this = if kind.needs_this { bind_this(stage, words.receiver, kind.sloppy, miss)? } else { words.receiver };
    if !kind.family.plain_header() {
        return Some(Entered { window: words.top, this });
    }
    fill_window(stage, words.top, words.registers, WindowFill { first: kind.first, argc: kind.argc, registers: kind.registers });
    let at = Region::Frame.mutable();
    stage.store_u64(at, words.top, context_at(frame::CONTEXT_THIS), this);
    stage.store_u64(at, words.top, context_at(frame::CONTEXT_ENV), words.env);
    stage.store_u64(at, words.top, context_at(frame::CONTEXT_CAPTURE), words.capture);
    let packed = stage.constant(kind.packed());
    let header = PlainHeader { call: words.call, caller_registers: words.registers, caller_base: words.base, packed, callee: words.held };
    push_plain_at(stage, words.plain, words.top, header, words.program, words.depth);
    Some(Entered { window: words.top, this })
}

/// The mirror of `enter`: the callee's outcome is guarded (`complete_call`), then the innermost
/// plain activation at `window` is popped (`pop_plain`). A refused outcome leaves at `miss` and
/// pops nothing.
#[allow(dead_code)] // C2-C5 route paths through it
#[inline(always)]
pub(crate) fn complete<S: Stage>(stage: &mut S, plain: S::Word, window: S::Word, outcome: CallOutcome<S::Word>, miss: S::Miss)
    -> Option<Completed<S::Word>> {
    let value = complete_call(stage, outcome.status, outcome.value, outcome.receiver, outcome.returned, miss)?;
    let caller = pop_plain(stage, plain, window);
    Some(Completed { value, caller })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;
    use crate::Value;

    const RETURNED: u32 = 7;
    const HELD: u64 = 0xF00D;
    const ENV: u64 = 0xE0E0;
    const CAPTURE: u64 = 0xCA9E;
    const CALL: u64 = 0xCA11;
    const BASE: u64 = 0xBA5E;
    const PROGRAM: u32 = 3;
    const GLOBAL: u64 = 0x61_0BA1;

    fn plain_kind(family: Family) -> CallKind {
        CallKind {
            family,
            function: 5,
            argc: 2,
            registers: 4,
            first: 0,
            needs_this: false,
            sloppy: None,
            callee: Some(HELD),
            flags: 0,
            returned: RETURNED,
        }
    }

    /// The window's register 0 is `frame[16]`; the context words are below it, the header's
    /// words in `frame[8..13]`. `plain` holds depth, registers and program, as `PlainFrames`.
    fn words(plain: &mut [u64; 4], frame: &mut [u64; 32], caller: &[u64; 4], receiver: u64) -> EnterWords<u64> {
        EnterWords {
            held: HELD,
            receiver,
            env: ENV,
            capture: CAPTURE,
            depth: plain[0],
            bound: 8,
            free: 100,
            needed: 10,
            plain: plain.as_mut_ptr() as u64,
            top: std::ptr::addr_of_mut!(frame[16]) as u64,
            registers: caller.as_ptr() as u64,
            base: BASE,
            call: CALL,
            program: u64::from(PROGRAM),
        }
    }

    fn run(kind: CallKind, words: EnterWords<u64>) -> Option<Entered<u64>> {
        enter(&mut Concrete::new(std::ptr::null()), kind, words, ())
    }

    #[test]
    fn a_plain_call_admits_fills_and_pushes_its_header() {
        let (mut plain, mut frame) = ([0_u64; 4], [0_u64; 32]);
        let caller = [10_u64, 20, 30, 40];
        let receiver = Value::UNDEFINED.to_bits();
        let kind = plain_kind(Family::Plain);
        let entered = run(kind, words(&mut plain, &mut frame, &caller, receiver)).expect("admitted");
        assert_eq!(entered.window, std::ptr::addr_of!(frame[16]) as u64);
        assert_eq!(entered.this, receiver, "a plain call binds nothing");
        assert_eq!(&frame[16..20], &[10, 20, Value::UNDEFINED.to_bits(), Value::UNDEFINED.to_bits()]);
        assert_eq!(frame[15], receiver, "the context words");
        assert_eq!(frame[14], ENV);
        assert_eq!(frame[13], CAPTURE);
        assert_eq!(frame[12], CALL, "the header: call, caller registers, caller base, packed, callee");
        assert_eq!(frame[11], caller.as_ptr() as u64);
        assert_eq!(frame[10], BASE);
        assert_eq!(frame[9], kind.packed());
        assert_eq!(frame[8], HELD);
        assert_eq!((plain[0], plain[1], plain[2] as u32), (1, entered.window, PROGRAM), "the depth, window and program");
    }

    #[test]
    fn a_method_call_binds_its_receiver_by_the_sloppy_rule() {
        let kind = CallKind { family: Family::Method, needs_this: true, sloppy: Some(GLOBAL), ..plain_kind(Family::Method) };
        let caller = [10_u64, 20, 30, 40];

        let (mut plain, mut frame) = ([0_u64; 4], [0_u64; 32]);
        let entered = run(kind, words(&mut plain, &mut frame, &caller, Value::UNDEFINED.to_bits())).expect("admitted");
        assert_eq!(entered.this, GLOBAL, "undefined binds the global object");
        assert_eq!(frame[15], GLOBAL, "the bound receiver is the callee's `this` word");

        let (mut plain, mut frame) = ([0_u64; 4], [0_u64; 32]);
        let boxed = Value::integer(5).to_bits();
        assert!(run(kind, words(&mut plain, &mut frame, &caller, boxed)).is_none(), "a primitive is the host's to box");
        assert_eq!(plain[0], 0, "a refused call pushes nothing");

        let strict = CallKind { sloppy: None, ..kind };
        let (mut plain, mut frame) = ([0_u64; 4], [0_u64; 32]);
        let entered = run(strict, words(&mut plain, &mut frame, &caller, boxed)).expect("strict takes any receiver");
        assert_eq!(entered.this, boxed);
    }

    #[test]
    fn a_refused_identity_or_depth_leaves_with_nothing_written() {
        let caller = [10_u64, 20, 30, 40];
        let (mut plain, mut frame) = ([0_u64; 4], [0_u64; 32]);
        let other = CallKind { callee: Some(HELD + 1), ..plain_kind(Family::Plain) };
        assert!(run(other, words(&mut plain, &mut frame, &caller, 0)).is_none(), "another callee leaves");

        let (mut plain, mut frame) = ([0_u64; 4], [0_u64; 32]);
        let full = EnterWords { bound: 0, ..words(&mut plain, &mut frame, &caller, 0) };
        assert!(run(plain_kind(Family::Plain), full).is_none(), "a plain depth at the bound leaves");
        assert_eq!((plain[0], frame), ([0; 4], [0; 32]), "no word is written before the guards pass");
    }

    #[test]
    fn a_completion_is_guarded_then_pops_the_header() {
        let caller = [10_u64, 20, 30, 40];
        let (mut plain, mut frame) = ([0_u64; 4], [0_u64; 32]);
        let entered = run(plain_kind(Family::Plain), words(&mut plain, &mut frame, &caller, 0)).expect("admitted");
        let plain_word = plain.as_mut_ptr() as u64;
        let outcome = |status, value, receiver| CallOutcome { status, value, receiver, returned: RETURNED };

        let refused = complete(&mut Concrete::new(std::ptr::null()), plain_word, entered.window,
            outcome(u64::from(RETURNED) + 1, 99, None), ());
        assert!(refused.is_none());
        assert_eq!(plain[0], 1, "a refused completion pops nothing");

        let done = complete(&mut Concrete::new(std::ptr::null()), plain_word, entered.window,
            outcome(u64::from(RETURNED), 99, None), ()).expect("returned");
        assert_eq!(done.value, 99);
        assert_eq!(done.caller.registers, caller.as_ptr() as u64, "the caller's window is resumed");
        assert_eq!((done.caller.call, done.caller.base), (CALL, BASE));
        assert_eq!(plain[0], 0, "the header is popped");
    }

    #[test]
    fn a_construction_completes_with_its_receiver() {
        let caller = [10_u64, 20, 30, 40];
        let (mut plain, mut frame) = ([0_u64; 4], [0_u64; 32]);
        let entered = run(plain_kind(Family::Plain), words(&mut plain, &mut frame, &caller, 0)).expect("admitted");
        let plain_word = plain.as_mut_ptr() as u64;
        let outcome = CallOutcome { status: u64::from(RETURNED), value: 99, receiver: Some(42), returned: RETURNED };
        let done = complete(&mut Concrete::new(std::ptr::null()), plain_word, entered.window, outcome, ()).expect("returned");
        assert_eq!(done.value, 42, "a construction yields its receiver, not the initializer's value");
    }
}
