//! Composite call row of the leaf call family (call protocol v2): composed from the staged
//! protocol rows (`protocol`), Stage-generic, no alloc or std, so every tier runs the same rows.
//!
//! A leaf call runs its callee's records on a window at the value stack's top without pushing an
//! activation: the call's facts live in the leaf record (the lane context's `LeafCall`) until the
//! callee's return completes the call (`complete_leaf`) or something observing activations makes
//! the leaf the activation its call would have pushed (`materialize_leaf_context` for the words
//! the leaf left to its caller). The steps it shares with the plain protocol:
//!
//! - callee identity: the tier's linked target (the interpreter's `linked_script_call_entry`),
//!   then its entry form, a per-site constant;
//! - admission: `call::admit_room` (parameters against arguments, room against the window), the
//!   tier's (`lane_admits_window` on `Concrete`);
//! - `this`: a form binding its receiver takes `protocol::bind_this`'s value; any other form only
//!   proves the binding is one of the receiver's own bits (`guard_receiver_binds`), made at
//!   materialization;
//! - the window: `protocol::fill_window`'s contract (arguments then `undefined`), which the
//!   interpreter fills with its admitted-room block stores;
//! - the frame push: replaced by the leaf record (`enter_leaf`), the one family-specific step;
//! - completion: the leaf record's caller (`complete_leaf`) in place of `protocol::pop_plain`'s.

use super::frame;
use super::protocol::bind_this;
use super::region::Region;
use super::Stage;
use crate::Value;

/// What a leaf's records read of the context its call runs them in: which of the receiver and
/// the environment the call makes the callee's. `NOT_LEAF` is a record of no leaf's records.
pub(crate) type LeafForm = u8;
pub const NOT_LEAF: LeafForm = 0;
/// Neither its receiver nor its captures: it runs in its caller's context.
pub const PLAIN_LEAF: LeafForm = 1;
/// Its captures and not its receiver: the call makes the environment its own.
pub const CAPTURE_LEAF: LeafForm = 2;
/// Its receiver, and maybe its captures: the call makes the receiver and the environment its own.
pub const CONTEXT_LEAF: LeafForm = 3;
/// Its receiver and not its captures: the call binds the receiver and keeps the caller's environment.
pub const THIS_LEAF: LeafForm = 4;

/// Whether a leaf of `form` runs with its receiver bound (`THIS_LEAF`, `CONTEXT_LEAF`).
#[inline(always)]
pub const fn binds_this(form: LeafForm) -> bool {
    form == CONTEXT_LEAF || form == THIS_LEAF
}

/// Whether a leaf of `form` runs in its own environment (`CAPTURE_LEAF`, `CONTEXT_LEAF`).
#[inline(always)]
pub(crate) const fn swaps_env(form: LeafForm) -> bool {
    form == CONTEXT_LEAF || form == CAPTURE_LEAF
}

/// The leaf record's words (the lane context's `LeafCall`, `#[repr(C)]`, which asserts these):
/// the caller's call record (0 while no leaf runs), registers and records, the target the call
/// resolved, the values it passed (u16) and its form (u8). The record's other words (the
/// caller's program of a crossing leaf) are the tier's.
pub const LEAF_CALL_AT: i32 = 0;
pub const LEAF_CALLER_REGISTERS_AT: i32 = 8;
pub const LEAF_CALLER_BASE_AT: i32 = 16;
pub const LEAF_TARGET_AT: i32 = 24;
pub const LEAF_ARGC_AT: i32 = 32;
pub const LEAF_FORM_AT: i32 = 34;

/// The byte offset of context word `word` below a window's register 0.
#[inline(always)]
const fn context_at(word: usize) -> i32 { -((word * 8) as i32) }

/// What a leaf call records in place of its activation (`enter_leaf`).
#[derive(Clone, Copy)]
pub struct LeafEntry<W> {
    /// The caller's call record.
    pub call: W,
    pub caller_registers: W,
    /// The caller's records (the lane context's `base`).
    pub caller_base: W,
    /// The resolved target, which stays cached while the leaf runs.
    pub target: W,
    /// The values the call passed.
    pub argc: W,
    /// The form the call runs as, a per-site constant.
    pub form: LeafForm,
}

/// The context words of the callee's window a leaf call may write: its bound receiver and its
/// environment and captures (the target's).
#[derive(Clone, Copy)]
pub struct LeafContext<W> {
    pub this: W,
    pub env: W,
    pub capture: W,
}

/// What a leaf's return reads of its record: the caller's call record, registers and records.
#[derive(Clone, Copy)]
pub struct LeafCaller<W> {
    pub call: W,
    pub registers: W,
    pub base: W,
}

/// Continues where the `this` of a callee for `receiver` is decided by the receiver's own bits
/// without a value to bind now: any binding but a sloppy one's, and a sloppy one's of
/// `undefined` (tested first, the commonest receiver of a plain call), an object of a direct
/// family or `null` (`protocol::bind_this`'s guard). Any other receiver (a primitive to box)
/// leaves at `miss`. Reads and writes no memory.
#[inline(always)]
pub fn guard_receiver_binds<S: Stage>(stage: &mut S, receiver: S::Word, sloppy: bool, miss: S::Miss) -> Option<()> {
    if !sloppy {
        return Some(());
    }
    let undefined = stage.eq_imm(receiver, Value::UNDEFINED.to_bits());
    if stage.known(undefined) == Some(true) {
        return Some(());
    }
    // The global is never bound here, so any constant stands for it.
    bind_this(stage, receiver, Some(Value::UNDEFINED.to_bits()), miss).map(|_| ())
}

/// Make the call `entry` the running leaf: its record at `leaf` (`LEAF_*_AT`, region `Dispatch`)
/// takes the caller's call record, registers and records, the target, the values passed and the
/// form; the callee's window at `window` (region `Frame`) takes `context.this` at
/// `frame::CONTEXT_THIS` where the form binds its receiver, and `context.env` and
/// `context.capture` at `frame::CONTEXT_ENV`/`CONTEXT_CAPTURE` where it swaps the environment.
/// Reads nothing. The tier fills the window and moves its own registers and records to the
/// callee's; the stack's top stays where it is until the leaf materializes.
#[inline(always)]
pub fn enter_leaf<S: Stage>(stage: &mut S, leaf: S::Word, window: S::Word, entry: LeafEntry<S::Word>, context: LeafContext<S::Word>) {
    let frame = Region::Frame.mutable();
    if binds_this(entry.form) {
        stage.store_u64(frame, window, context_at(frame::CONTEXT_THIS), context.this);
    }
    let record = Region::Dispatch.mutable();
    stage.store_u64(record, leaf, LEAF_CALL_AT, entry.call);
    stage.store_u64(record, leaf, LEAF_CALLER_REGISTERS_AT, entry.caller_registers);
    stage.store_u64(record, leaf, LEAF_CALLER_BASE_AT, entry.caller_base);
    stage.store_u64(record, leaf, LEAF_TARGET_AT, entry.target);
    stage.store_u16(record, leaf, LEAF_ARGC_AT, entry.argc);
    let form = stage.constant(u64::from(entry.form));
    stage.store_u8(record, leaf, LEAF_FORM_AT, form);
    if swaps_env(entry.form) {
        stage.store_u64(frame, window, context_at(frame::CONTEXT_ENV), context.env);
        stage.store_u64(frame, window, context_at(frame::CONTEXT_CAPTURE), context.capture);
    }
}

/// The return of the running leaf whose record is at `leaf`: its caller's call record,
/// registers and records, read from the record (`LEAF_CALL_AT`, `LEAF_CALLER_*_AT`), and the
/// record's call word cleared, so no leaf runs. Leaves at `miss` where none runs (the call word
/// is 0), changing nothing. The tier restores its registers and records to the caller's, writes
/// the call's value to the record's result register and continues past the call record; a
/// crossing leaf's tier enters the caller's program again.
#[inline(always)]
pub fn complete_leaf<S: Stage>(stage: &mut S, leaf: S::Word, miss: S::Miss) -> Option<LeafCaller<S::Word>> {
    let record = Region::Dispatch.mutable();
    let call = stage.load_u64(record, leaf, LEAF_CALL_AT);
    let running = stage.ne_imm(call, 0u64);
    stage.guard(running, miss)?;
    let none = stage.constant(0);
    stage.store_u64(record, leaf, LEAF_CALL_AT, none);
    let registers = stage.load_u64(record, leaf, LEAF_CALLER_REGISTERS_AT);
    let base = stage.load_u64(record, leaf, LEAF_CALLER_BASE_AT);
    Some(LeafCaller { call, registers, base })
}

/// A tier's fill of a leaf call's window: the values the call passed, then `undefined` up to the
/// callee's registers (`protocol::fill_window`'s contract), written however the tier stores
/// best (the interpreter's admitted-room block stores).
pub trait LeafWindow<S: Stage> {
    fn fill(&mut self, stage: &mut S, window: S::Word);
}

/// The leaf call of `entry`: the callee's window at `window` filled (`LeafWindow`) and the
/// call made the running leaf (`enter_leaf`). The one step of every leaf call: the lane's
/// call records of a leaf target, a crossing leaf's, a getter's leaf and a cell body's leaf
/// run it. The tier moves its registers and records to the callee's.
#[inline(always)]
pub fn step_leaf_call_of<S: Stage, F: LeafWindow<S>>(stage: &mut S, window_fill: &mut F, leaf: S::Word, window: S::Word,
    entry: LeafEntry<S::Word>, context: LeafContext<S::Word>) {
    window_fill.fill(stage, window);
    enter_leaf(stage, leaf, window, entry, context);
}

/// The return of the running leaf with `value`: its caller (`complete_leaf`), with `value` in
/// the call's result register, at the byte offset `result_at` gives for the caller's call record.
/// The one step of every leaf return. Leaves at `miss` where no leaf runs, changing nothing.
#[inline(always)]
pub fn step_leaf_return_of<S: Stage>(stage: &mut S, leaf: S::Word, value: S::Word,
    result_at: impl FnOnce(&mut S, S::Word) -> S::Word, miss: S::Miss) -> Option<LeafCaller<S::Word>> {
    let caller = complete_leaf(stage, leaf, miss)?;
    let offset = result_at(stage, caller.call);
    let result = stage.add(caller.registers, offset);
    stage.store_u64(Region::Frame.mutable(), result, 0, value);
    Some(caller)
}

/// The context words a leaf of `form` left to its caller, written into its window at `window`
/// (region `Frame`) when it becomes the activation its call would have pushed: `this`, the
/// binding its call proved (`guard_receiver_binds`), at `frame::CONTEXT_THIS` where the form
/// binds no receiver, which is exactly where the tier passes one; `env` and `capture` (the
/// target's) at `frame::CONTEXT_ENV`/`CONTEXT_CAPTURE` where it keeps the caller's environment.
/// Reads nothing. The tier pushes the activation itself (`protocol::push_plain` for a plain
/// completion).
#[inline(always)]
pub fn materialize_leaf_context<S: Stage>(stage: &mut S, window: S::Word, form: LeafForm, this: Option<S::Word>, env: S::Word, capture: S::Word) {
    debug_assert_eq!(this.is_some(), !binds_this(form), "a form binding no receiver binds it at materialization");
    let frame = Region::Frame.mutable();
    if let Some(this) = this {
        stage.store_u64(frame, window, context_at(frame::CONTEXT_THIS), this);
    }
    if !swaps_env(form) {
        stage.store_u64(frame, window, context_at(frame::CONTEXT_ENV), env);
        stage.store_u64(frame, window, context_at(frame::CONTEXT_CAPTURE), capture);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;

    fn concrete() -> Concrete { Concrete::new(std::ptr::null()) }

    #[test]
    fn a_strict_receiver_always_binds_and_a_sloppy_primitive_never_does() {
        let mut heap = crate::heap::Heap::new();
        let object = heap.alloc_object(crate::heap::Object::new(Value::NULL));
        let binds = |receiver: Value, sloppy| guard_receiver_binds(&mut concrete(), receiver.to_bits(), sloppy, ()).is_some();
        for value in [object, Value::UNDEFINED, Value::NULL, Value::integer(1)] {
            assert!(binds(value, false), "{value:?}");
            assert_eq!(binds(value, true), value.is_undefined() || value.is_null() || value.is_direct_object_family(), "{value:?}");
        }
    }

    /// The record's words and the window's context words, entered, completed and materialized.
    #[test]
    fn a_leaf_enters_completes_and_materializes_through_its_record() {
        let mut record = [0_u64; 6];
        let mut window = [0_u64; frame::CONTEXT_WORDS + 2];
        let leaf = record.as_mut_ptr() as u64;
        let registers = unsafe { window.as_mut_ptr().add(frame::CONTEXT_WORDS) } as u64;
        let entry = LeafEntry { call: 11, caller_registers: 12, caller_base: 13, target: 14, argc: 2, form: CAPTURE_LEAF };
        enter_leaf(&mut concrete(), leaf, registers, entry, LeafContext { this: 21, env: 22, capture: 23 });
        assert_eq!(&record[..4], &[11, 12, 13, 14]);
        assert_eq!(record[4] & 0xFF_FFFF, 2 | (u64::from(CAPTURE_LEAF) << 16));
        let context = |word: usize| window[frame::CONTEXT_WORDS - word];
        assert_eq!((context(frame::CONTEXT_THIS), context(frame::CONTEXT_ENV), context(frame::CONTEXT_CAPTURE)), (0, 22, 23));
        materialize_leaf_context(&mut concrete(), registers, CAPTURE_LEAF, Some(31), 32, 33);
        assert_eq!((context(frame::CONTEXT_THIS), context(frame::CONTEXT_ENV)), (31, 22), "a capture leaf's environment is its own already");
        let caller = complete_leaf(&mut concrete(), leaf, ()).expect("the leaf runs");
        assert_eq!((caller.call, caller.registers, caller.base), (11, 12, 13));
        assert_eq!(record[0], 0);
        assert!(complete_leaf(&mut concrete(), leaf, ()).is_none(), "no leaf runs once completed");
    }
}
