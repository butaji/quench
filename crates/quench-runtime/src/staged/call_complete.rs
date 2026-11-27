//! The completion words of a call's return as staged rows (call protocol v2): what the return of
//! a callee writes into its caller's result register, and the call record's bits that decide
//! whether a return completes its caller at all. Stage-generic, no alloc or std, so every tier
//! runs the same rows.
//!
//! The interpreter's completion helpers (`vm/dispatch_fast/completions.rs`) run these rows on
//! `Concrete`. The activation records, the lane's state and the heap work of a completion stay
//! the interpreter's own; a row takes the words those read and yields the word they write.

use super::region::Region;
use super::{frame, word_offset, Stage};
use crate::bytecode::{Register, RETURN_REGISTER};
use crate::Value;

/// Whether a call record whose result register word is `call` returns its callee's completion
/// (`RETURN_REGISTER` set): the completion of the callee then completes this call in turn.
/// Reads `call` only; writes nothing.
#[inline(always)]
pub fn returns_callee_completion<S: Stage>(stage: &mut S, call: S::Word) -> S::Cond {
    let bit = stage.and_imm(call, u64::from(RETURN_REGISTER));
    stage.ne_imm(bit, 0u64)
}

/// `returns_callee_completion` on machine words, for the interpreter's call records.
#[inline(always)]
pub fn concrete_returns_callee_completion(result: Register) -> bool {
    returns_callee_completion(&mut super::Concrete::new(std::ptr::null()), u64::from(result))
}

/// The caller's result register `result` takes `value` in the window at `registers`: the call's
/// value, which the caller resumes with. Writes one word of the frame region; reads nothing else.
#[inline(always)]
pub(crate) fn write_call_result<S: Stage>(stage: &mut S, registers: S::Word, result: Register, value: S::Word) {
    stage.store_u64(Region::Frame.mutable(), registers, word_offset(result), value);
}

/// `write_call_result` on machine words: `registers` is the caller's window address.
#[inline(always)]
pub fn concrete_write_call_result(registers: u64, result: Register, value: Value) {
    write_call_result(&mut super::Concrete::new(std::ptr::null()), registers, result, value.to_bits());
}

/// The value the caller's result register `result` holds in the window at `registers`: a
/// setter's store yields it (`Completion::Assigns`), the value its caller stored. Reads one word.
#[inline(always)]
pub(crate) fn call_result<S: Stage>(stage: &mut S, registers: S::Word, result: Register) -> S::Word {
    stage.load_u64(Region::Frame.readonly(), registers, word_offset(result))
}

/// `call_result` on machine words.
#[inline(always)]
pub fn concrete_call_result(registers: u64, result: Register) -> Value {
    Value::from_bits(call_result(&mut super::Concrete::new(std::ptr::null()), registers, result))
}

/// The receiver of the frame returning from the window at `window`, its `this` context word
/// (`frame::CONTEXT_THIS`): what a construction yields for a non-object, a body's construct
/// completion takes, and the caller's restore hands back. Reads one word of the frame region.
#[inline(always)]
pub(crate) fn returning_this<S: Stage>(stage: &mut S, window: S::Word) -> S::Word {
    stage.load_u64(Region::Frame.readonly(), window, -((frame::CONTEXT_THIS * 8) as i32))
}

/// `returning_this` on a machine word.
#[inline(always)]
pub fn concrete_returning_this(window: u64) -> Value {
    Value::from_bits(returning_this(&mut super::Concrete::new(std::ptr::null()), window))
}

/// Whether the return of a frame whose packed word is `packed` into the call record whose
/// result word is `destination` is the header's plain return (`protocol::pop_plain`, then
/// `write_call_result`): no side flag, and a call that keeps its result, or, where `into_body`
/// (a lane body's call, which completes its own activation next), a returning one too. Any
/// other return is a side record's completion or a returning call's chain. Reads the two words.
#[inline(always)]
pub(crate) fn plain_return_admits<S: Stage>(stage: &mut S, packed: S::Word, destination: S::Word, into_body: S::Cond) -> S::Cond {
    let plain = completes_plainly(stage, packed);
    let returning = returns_callee_completion(stage, destination);
    let keeps = stage.not(returning);
    let completes = stage.any(keeps, into_body);
    stage.all(plain, completes)
}

/// The plain return of the frame at `window` with `value` (call protocol v2 stage C), `plain` the
/// lane's plain-frame words: where a plain frame runs (its depth is not 0) and the return is the
/// header's (`plain_return_admits` of the result word `destination` reads from the caller's call
/// record), the header pops (`protocol::pop_plain`) and the caller's result register takes `value`.
/// The one step of every plain return; the tier releases the window. Leaves at `miss`, changing
/// nothing, for any other return.
#[inline(always)]
pub fn step_plain_return_of<S: Stage>(stage: &mut S, plain: S::Word, window: S::Word, value: S::Word, into_body: S::Cond,
    destination: impl FnOnce(&mut S, S::Word) -> S::Word, miss: S::Miss) -> Option<super::protocol::PlainCaller<S::Word>> {
    let depth = stage.load_u64(Region::Payload.mutable(), plain, frame::PLAIN_DEPTH_AT as i32);
    let running = stage.ne_imm(depth, 0u64);
    stage.guard(running, miss)?;
    let call = stage.load_u64(Region::Frame.mutable(), window, -word_offset(frame::CONTEXT_CALL as u16));
    let destination = destination(stage, call);
    let packed = frame_packed(stage, window);
    let admits = plain_return_admits(stage, packed, destination, into_body);
    stage.guard(admits, miss)?;
    let caller = super::protocol::pop_plain(stage, plain, window);
    let register = stage.and_imm(destination, u64::from(crate::bytecode::REGISTER_MASK));
    let offset = stage.mul_imm(register, super::word_bytes(1));
    let result = stage.add(caller.registers, offset);
    stage.store_u64(Region::Frame.mutable(), result, 0, value);
    Some(caller)
}

/// The pop of the plain frame at `window` whose call returns its callee's completion
/// (`return f()`), `plain` the lane's plain-frame words: where a plain frame runs, its header
/// completes it (`completes_plainly`) and its call record's result word `destination` returns the
/// callee's completion, the header pops (`protocol::pop_plain`) and the caller, which completes
/// with the same value, runs next. A run of such frames pops one step each; the tier releases the
/// windows. Leaves at `miss`, changing nothing, for any other frame.
#[inline(always)]
pub fn step_returning_pop_of<S: Stage>(stage: &mut S, plain: S::Word, window: S::Word,
    destination: impl FnOnce(&mut S, S::Word) -> S::Word, miss: S::Miss) -> Option<super::protocol::PlainCaller<S::Word>> {
    let depth = stage.load_u64(Region::Payload.mutable(), plain, frame::PLAIN_DEPTH_AT as i32);
    let running = stage.ne_imm(depth, 0u64);
    stage.guard(running, miss)?;
    let packed = frame_packed(stage, window);
    let plainly = completes_plainly(stage, packed);
    stage.guard(plainly, miss)?;
    let call = stage.load_u64(Region::Frame.mutable(), window, -word_offset(frame::CONTEXT_CALL as u16));
    let destination = destination(stage, call);
    let returning = returns_callee_completion(stage, destination);
    stage.guard(returning, miss)?;
    Some(super::protocol::pop_plain(stage, plain, window))
}

/// Whether a settling call's return settles its promise in the lane: unless the host observes
/// promise lifecycles, whose hooks may run guest code, so the frame's pending call settles it.
/// Reads the fact; writes nothing.
#[inline(always)]
pub(crate) fn settles_in_lane<S: Stage>(stage: &mut S, observes_lifecycle: S::Cond) -> S::Cond {
    stage.not(observes_lifecycle)
}

/// `settles_in_lane` on a machine fact.
#[inline(always)]
pub fn concrete_settles_in_lane(observes_lifecycle: bool) -> bool {
    settles_in_lane(&mut super::Concrete::new(std::ptr::null()), observes_lifecycle)
}

/// Whether a settling call's return yields its value with no promise: a call whose result only
/// the `await` after it reads (no rooted promise) returning a primitive, which awaiting
/// fulfills as the promise would. Reads the two facts; writes nothing.
#[inline(always)]
pub(crate) fn settles_without_promise<S: Stage>(stage: &mut S, rooted: S::Cond, object: S::Cond) -> S::Cond {
    let either = stage.any(rooted, object);
    stage.not(either)
}

/// `settles_without_promise` on machine facts.
#[inline(always)]
pub fn concrete_settles_without_promise(rooted: bool, object: bool) -> bool {
    settles_without_promise(&mut super::Concrete::new(std::ptr::null()), rooted, object)
}

/// Whether a returning call is admitted to the lane: it is, unless the caller is strict, since a strict
/// caller's terminal call is a proper tail call (`call_plan` tail sites), which the general dispatch path
/// takes as a jump. The lane has no frame reuse, so it cannot take that call itself. Reads one fact; writes nothing.
#[inline(always)]
pub(crate) fn admits_returning_call<S: Stage>(stage: &mut S, strict: S::Cond) -> S::Cond {
    stage.not(strict)
}

/// `admits_returning_call` on machine words.
#[inline(always)]
pub fn concrete_admits_returning_call(strict: bool) -> bool {
    admits_returning_call(&mut super::Concrete::new(std::ptr::null()), strict)
}

/// The packed word of the frame whose window is `window` (`frame::CONTEXT_PACKED`, call protocol
/// v2 stage C): function, flags and argc. Reads one word of the frame region.
#[inline(always)]
pub fn frame_packed<S: Stage>(stage: &mut S, window: S::Word) -> S::Word {
    stage.load_u64(Region::Frame.readonly(), window, -((frame::CONTEXT_PACKED * 8) as i32))
}

/// Whether a frame whose packed word is `packed` completes from its header alone: no side-record
/// flag (`frame::SIDE_FLAGS`), so its return is the plain pop (`protocol::pop_plain`) and nothing
/// more (one-frame-one-call.md F4). Any flag selects the side record's completion.
#[inline(always)]
pub fn completes_plainly<S: Stage>(stage: &mut S, packed: S::Word) -> S::Cond {
    let sided = stage.and_imm(packed, frame::SIDE_FLAGS);
    stage.eq_imm(sided, 0u64)
}

/// `completes_plainly` of the frame at `window`, its packed word loaded: the one test the plain
/// return adds for frames that carry a side record.
#[inline(always)]
pub(crate) fn concrete_frame_completes_plainly(window: u64) -> bool {
    let stage = &mut super::Concrete::new(std::ptr::null());
    let packed = frame_packed(stage, window);
    completes_plainly(stage, packed)
}

/// The side-record flags of a frame (one-frame-one-call.md F4): `keeps` (passed values past its
/// registers), `env` (environment entries and locals in its window), `crosses` (its caller runs
/// another program), `completes` (a completion past `Plain` and `Cross`) and `owns` (an eval
/// record its side record names, one-frame-one-call.md F4 (3)). Zero for a frame whose header
/// is all of it. Writes nothing.
#[inline(always)]
pub(crate) fn side_flags<S: Stage>(stage: &mut S, keeps: S::Cond, env: S::Cond, crosses: S::Cond, completes: S::Cond, owns: S::Cond) -> S::Word {
    let none = stage.constant(0);
    let keeps_flag = stage.constant(frame::KEEPS_FLAG);
    let env_flag = stage.constant(frame::ENV_FLAG);
    let crosses_flag = stage.constant(frame::CROSSES_FLAG);
    let completes_flag = stage.constant(frame::COMPLETION_FLAG);
    let owns_flag = stage.constant(frame::OWNS_FLAG);
    let keeps = stage.select(keeps, keeps_flag, none);
    let env = stage.select(env, env_flag, none);
    let crosses = stage.select(crosses, crosses_flag, none);
    let completes = stage.select(completes, completes_flag, none);
    let owns = stage.select(owns, owns_flag, none);
    let flags = stage.or(keeps, env);
    let flags = stage.or(flags, crosses);
    let flags = stage.or(flags, completes);
    stage.or(flags, owns)
}

/// `side_flags` on machine facts.
#[inline(always)]
pub fn concrete_side_flags(keeps: bool, env: bool, crosses: bool, completes: bool, owns: bool) -> u64 {
    side_flags(&mut super::Concrete::new(std::ptr::null()), keeps, env, crosses, completes, owns)
}

/// Write `flags` as the side-record flags of the frame header below `window`
/// (`frame::SIDE_FLAGS`), its function, other flags and argc kept: a frame given another
/// completion after its push (a setter's store, a trap, a callback, a Wasm import's callee).
/// Reads and writes the packed word.
#[inline(always)]
pub(crate) fn write_side_flags<S: Stage>(stage: &mut S, window: S::Word, flags: S::Word) {
    let packed = frame_packed(stage, window);
    let kept = stage.and_imm(packed, !frame::SIDE_FLAGS);
    let packed = stage.or(kept, flags);
    stage.store_u64(Region::Frame.mutable(), window, -((frame::CONTEXT_PACKED * 8) as i32), packed);
}

/// `write_side_flags` on machine words.
#[inline(always)]
pub fn concrete_write_side_flags(window: u64, flags: u64) {
    write_side_flags(&mut super::Concrete::new(std::ptr::null()), window, flags);
}

/// What a side record's completion runs past the header's pop, selected by the flags of the
/// popped frame's packed word: the caller's program entered again (`crosses`); keeping and
/// environment frames need nothing past the window's release.
#[derive(Clone, Copy)]
pub struct SideCompletion<C> {
    pub sided: C,
    pub crosses: C,
}

/// The side completion row: which side steps the frame of packed word `packed` runs once its
/// header is popped. Reads the word only.
#[inline(always)]
pub(crate) fn side_completion<S: Stage>(stage: &mut S, packed: S::Word) -> SideCompletion<S::Cond> {
    let sided = stage.and_imm(packed, frame::SIDE_FLAGS);
    let sided = stage.ne_imm(sided, 0u64);
    let crosses = stage.and_imm(packed, frame::CROSSES_FLAG);
    let crosses = stage.ne_imm(crosses, 0u64);
    SideCompletion { sided, crosses }
}

/// `side_completion` on a machine word.
#[inline(always)]
pub fn concrete_side_completion(packed: u64) -> SideCompletion<bool> {
    side_completion(&mut super::Concrete::new(std::ptr::null()), packed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;

    #[test]
    fn a_call_returns_its_callees_completion_by_the_return_bit_alone() {
        assert!(concrete_returns_callee_completion(RETURN_REGISTER | 3));
        assert!(!concrete_returns_callee_completion(3));
        assert!(!concrete_returns_callee_completion(0));
    }

    #[test]
    fn a_result_is_written_to_its_register_and_nothing_else() {
        let mut window = [0_u64; 4];
        let registers = window.as_mut_ptr() as u64;
        write_call_result(&mut Concrete::new(std::ptr::null()), registers, 2, 77);
        assert_eq!(window, [0, 0, 77, 0]);
        concrete_write_call_result(registers, 1, Value::integer(5));
        assert_eq!(window[1], Value::integer(5).to_bits());
    }

    #[test]
    fn a_frame_completes_plainly_without_a_side_flag() {
        use crate::staged::protocol::plain_packed;
        assert!(completes_plainly(&mut Concrete::new(std::ptr::null()), plain_packed(3, 2, 0)));
        assert!(!completes_plainly(&mut Concrete::new(std::ptr::null()), plain_packed(3, 2, frame::KEEPS_FLAG)));
        assert!(completes_plainly(&mut Concrete::new(std::ptr::null()), plain_packed(3, 2, frame::BODY_FLAG)), "a body flag is no side record");
        let mut words = [0_u64; 9];
        words[8 - frame::CONTEXT_PACKED] = plain_packed(1, 0, frame::CROSSES_FLAG);
        let window = words.as_mut_ptr().wrapping_add(8) as u64;
        assert!(!concrete_frame_completes_plainly(window));
    }

    #[test]
    fn side_flags_select_their_completion() {
        assert_eq!(concrete_side_flags(false, false, false, false, false), 0);
        assert_eq!(concrete_side_flags(false, false, false, false, true), frame::OWNS_FLAG);
        let flags = concrete_side_flags(true, false, true, false, false);
        assert_eq!(flags, frame::KEEPS_FLAG | frame::CROSSES_FLAG);
        let side = concrete_side_completion(crate::staged::protocol::plain_packed(4, 1, flags));
        assert!(side.sided && side.crosses);
        let side = concrete_side_completion(crate::staged::protocol::plain_packed(4, 1, frame::ENV_FLAG | frame::COMPLETION_FLAG));
        assert!(side.sided && !side.crosses);
        assert!(!concrete_side_completion(crate::staged::protocol::plain_packed(4, 1, 0)).sided);
    }

    #[test]
    fn the_header_return_admits_a_plain_frame_into_a_keeping_call() {
        use crate::staged::protocol::plain_packed;
        let plain = plain_packed(3, 1, 0);
        let sided = plain_packed(3, 1, frame::ENV_FLAG);
        let run = |packed, destination, into_body| plain_return_admits(&mut Concrete::new(std::ptr::null()), packed, u64::from(destination), into_body);
        assert!(run(plain, 2, false));
        assert!(!run(plain, RETURN_REGISTER | 2, false), "a returning call takes the chain");
        assert!(run(plain, RETURN_REGISTER | 2, true), "a body's call completes its own activation next");
        assert!(!run(sided, 2, true), "a side record completes past the pop");
        let mut words = [0_u64; 9];
        words[8 - frame::CONTEXT_PACKED] = plain;
        words[8 - frame::CONTEXT_THIS] = 41;
        let window = words.as_mut_ptr().wrapping_add(8) as u64;
        assert_eq!(concrete_returning_this(window).to_bits(), 41);
        let mut registers = [0_u64, 9];
        assert_eq!(concrete_call_result(registers.as_mut_ptr() as u64, 1).to_bits(), 9);
    }

    #[test]
    fn the_plain_return_step_pops_the_header_and_writes_the_caller_register() {
        let mut caller = [0_u64; 4];
        let mut words = [0_u64; 9];
        words[8 - frame::CONTEXT_PACKED] = crate::staged::protocol::plain_packed(3, 1, 0);
        words[8 - frame::CONTEXT_CALL] = 77;
        words[8 - frame::CONTEXT_CALLER_REGISTERS] = caller.as_mut_ptr() as u64;
        let window = words.as_mut_ptr().wrapping_add(8) as u64;
        let mut plain = [0_u64; 4];
        let run = |plain: &mut [u64; 4], destination: Register, into_body: bool| {
            let at = plain.as_mut_ptr() as u64;
            step_plain_return_of(&mut Concrete::new(std::ptr::null()), at, window, 9, into_body, |_, call| {
                assert_eq!(call, 77);
                u64::from(destination)
            }, ()).map(|caller| caller.call)
        };
        assert_eq!(run(&mut plain, 2, false), None, "no plain frame runs");
        plain[frame::PLAIN_DEPTH_AT / 8] = 1;
        assert_eq!(run(&mut plain, RETURN_REGISTER | 2, false), None, "a returning call takes the chain");
        assert_eq!(plain[frame::PLAIN_DEPTH_AT / 8], 1, "a declined return changes nothing");
        assert_eq!(run(&mut plain, 2, false), Some(77));
        assert_eq!((plain[frame::PLAIN_DEPTH_AT / 8], caller[2]), (0, 9));
    }

    #[test]
    fn a_settlement_stays_in_the_lane_unless_observed() {
        assert!(concrete_settles_in_lane(false));
        assert!(!concrete_settles_in_lane(true));
        assert!(concrete_settles_without_promise(false, false));
        assert!(!concrete_settles_without_promise(true, false));
        assert!(!concrete_settles_without_promise(false, true));
    }

    #[test]
    fn a_strict_caller_takes_the_general_path_whatever_the_host() {
        assert!(concrete_admits_returning_call(false), "a sloppy caller is admitted");
        assert!(!concrete_admits_returning_call(true), "a strict caller's terminal call is a tail call the lane cannot reuse");
    }
}
