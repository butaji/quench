//! Composite call row of the environment call family (call protocol v2): composed from the staged
//! protocol rows (`protocol`), Stage-generic, no alloc or std, so every tier runs the same rows.
//!
//! An environment callee (`CallEntry::Environment`) runs on a window that holds, after its
//! `registers` words, the environment's entries (`ENTRY_*`, `ENTRY_WORDS` words), then its
//! `locals`, then every value the call passed (kept for legacy reflection). The words below
//! register 0 are the frame context (`frame::CONTEXT_*`); an environment activation keeps a full
//! activation record, so only `this`, `env` and `capture` are written there, never a plain header.
//!
//! What stays the tier's own (host work no row expresses): the returning-call admission, the
//! callee plan's legacy-reflection test for values past the parameters, the `with` scope test,
//! the rest array's allocation, the values' top, and the activation record's push.

use super::call::admit_room;
use super::frame;
use super::region::Region;
use super::{word_bytes, Passed, Stage};
use crate::Value;

/// The environment entries after a window's registers, in words from the first of them: the
/// environment, `DELETED` until one is made; the address of its slot storage; the captured mask;
/// then the in-place slot storage's words (`EnvironmentSlots::lay_out_in_place`: the storage
/// word, which holds the address of the header word, and the header word, which holds the slot
/// count). The locals, the storage's slots, follow at `ENTRY_WORDS`. The interpreter asserts
/// these against `dispatch_fast::WINDOW_*` and `heap::IN_PLACE_SLOTS_WORDS`.
pub const ENTRY_ENVIRONMENT: u16 = 0;
pub const ENTRY_SLOTS: u16 = 1;
pub const ENTRY_CAPTURED: u16 = 2;
pub const ENTRY_STORAGE: u16 = 3;
/// The in-place storage's words: the storage word, then the header word (`ThinBoxSlice::IN_PLACE_WORDS`).
pub const STORAGE_WORDS: u16 = 2;
/// Where the locals start, counted from the first entry (`crate::vm::engine_types::WINDOW_LOCALS`).
pub const ENTRY_WORDS: u16 = ENTRY_STORAGE + STORAGE_WORDS;

/// The byte offset of word `index` from a base, for indices past `u16` (locals plus values).
#[inline(always)]
const fn words_at(index: u32) -> i32 { (index * std::mem::size_of::<u64>() as u32) as i32 }

/// The byte offset of context word `word` below a window's register 0.
#[inline(always)]
const fn context_at(word: usize) -> i32 { -((word * std::mem::size_of::<u64>()) as i32) }

/// Continues where an environment activation fits both stacks: `activations` records below
/// `limit` (the record stack's one more must fit), and `needed` words (the target's room plus the
/// values kept past its parameters) within the values' `free` words. Leaves at `miss` otherwise,
/// where the tier grows the stack in a frame (`lane_script_call_frame`).
///
/// Reads nothing; the tier loaded the four words.
#[inline(always)]
pub(crate) fn admit_environment<S: Stage>(stage: &mut S, (activations, limit): (S::Word, S::Word), (free, needed): (S::Word, S::Word),
    miss: S::Miss) -> Option<()> {
    let next = stage.add_imm(activations, 1u64);
    admit_room(stage, limit, next, miss)?;
    admit_room(stage, free, needed, miss)
}

/// `admit_environment` on machine words: the interpreter's admission of an environment window.
#[inline(always)]
pub fn concrete_admit_environment(activations: u64, limit: u64, free: u64, needed: u64) -> bool {
    admit_environment(&mut super::Concrete::new(std::ptr::null()), (activations, limit), (free, needed), ()).is_some()
}

/// Lays out the environment entries at `entries` (the word after the window's registers) over
/// `locals` slots living in place: `write_entries` of `DELETED` and the in-place storage at
/// `ENTRY_STORAGE`, then the storage's two words (the header's address, then `locals`). Reads
/// nothing.
#[inline(always)]
pub(crate) fn lay_out_entries<S: Stage>(stage: &mut S, entries: S::Word, captured: u64, locals: u16) {
    let at = Region::Frame.mutable();
    let deleted = stage.constant(Value::DELETED.to_bits());
    let storage = stage.add_imm(entries, word_bytes(ENTRY_STORAGE));
    write_entries(stage, entries, deleted, storage, captured);
    let header = stage.add_imm(storage, word_bytes(1));
    stage.store_u64(at, storage, 0, header);
    let count = stage.constant(u64::from(locals));
    stage.store_u64(at, header, 0, count);
}

/// Writes the three leading entries at `entries`: `ENTRY_ENVIRONMENT` (`environment`, `DELETED`
/// until one is made), `ENTRY_SLOTS` (`slots`, the slot storage's address) and `ENTRY_CAPTURED`
/// (`captured`). A window whose environment exists already (a resumed frame's own) writes these
/// alone; a fresh one lays out its in-place storage too (`lay_out_entries`). Reads nothing.
#[inline(always)]
pub(crate) fn write_entries<S: Stage>(stage: &mut S, entries: S::Word, environment: S::Word, slots: S::Word, captured: u64) {
    let at = Region::Frame.mutable();
    stage.store_u64(at, entries, words_at(u32::from(ENTRY_ENVIRONMENT)), environment);
    stage.store_u64(at, entries, words_at(u32::from(ENTRY_SLOTS)), slots);
    let captured = stage.constant(captured);
    stage.store_u64(at, entries, words_at(u32::from(ENTRY_CAPTURED)), captured);
}

/// Writes the `count` values at `arguments` into the window's locals at `local`: each of the
/// first `bound` past the first `in_registers` as its parameter's local, and every one after the
/// `locals`, as the value a frame keeps for legacy reflection. A parameter below `in_registers`
/// lives in its register (`EnvironmentPassed::in_registers`), and its local stays `undefined`, as
/// a frame clears a promoted parameter's slot (`Frame::place_parameters`). Reads the
/// `count` values; writes `count + min(count, bound) - min(count, in_registers)` words. (The
/// interpreter's copy of the passed values, on `Concrete`.)
#[inline(always)]
pub(crate) fn write_arguments<S: Stage>(stage: &mut S, local: S::Word, locals: u16, (arguments, count): (S::Word, u16),
    (in_registers, bound): (u16, u16)) {
    let at = Region::Frame.mutable();
    for slot in 0..u32::from(count) {
        let value = stage.load_u64(at, arguments, words_at(slot));
        if slot >= u32::from(in_registers) && slot < u32::from(bound) {
            stage.store_u64(at, local, words_at(slot), value);
        }
        stage.store_u64(at, local, words_at(u32::from(locals) + slot), value);
    }
}

/// Writes the frame context of a window at `registers`: `this`, `env` and `capture`
/// (`frame::CONTEXT_THIS`, `CONTEXT_ENV`, `CONTEXT_CAPTURE`). Reads nothing.
#[inline(always)]
pub(crate) fn bind_context<S: Stage>(stage: &mut S, registers: S::Word, context: WindowContext<S::Word>) {
    let at = Region::Frame.mutable();
    stage.store_u64(at, registers, context_at(frame::CONTEXT_ENV), context.env);
    stage.store_u64(at, registers, context_at(frame::CONTEXT_CAPTURE), context.capture);
    stage.store_u64(at, registers, context_at(frame::CONTEXT_THIS), context.this);
}

/// The frame context a callee runs with: its receiver, its closure's environment and captures.
#[derive(Clone, Copy)]
pub struct WindowContext<W> {
    pub(crate) this: W,
    pub(crate) env: W,
    /// The captures' address (`ScriptCallTarget::capture`), as a word.
    pub(crate) capture: W,
}

/// The shape of an environment window, from the target and its view (`EnvironmentWindow`).
#[derive(Clone, Copy)]
pub struct EnvironmentShape {
    /// The callee's registers (`ScriptCallTarget::registers`), after which the entries start.
    pub(crate) registers: u16,
    /// The registers live at entry, which start `undefined` (`ScriptCallTarget::entry_registers`).
    pub(crate) live: u16,
    /// The environment's slots, the window's locals (`EnvironmentWindow::locals`).
    pub(crate) locals: u16,
    /// The slots the environment owns once made (`EnvironmentWindow::captured`).
    pub(crate) captured: u64,
}

impl EnvironmentShape {
    /// The window's words before its kept values: registers, entries, locals.
    #[inline(always)]
    pub const fn held(self) -> usize {
        self.registers as usize + ENTRY_WORDS as usize + self.locals as usize
    }
}

/// The values an environment call passes to its window: `count` values at `arguments`, of which
/// the first `bound` are parameters, plus the locals the call binds itself: the rest array at its
/// parameter's local and the callee at a named function expression's own local.
#[derive(Clone, Copy)]
pub struct EnvironmentPassed<W> {
    pub(crate) arguments: W,
    pub(crate) count: u16,
    pub(crate) bound: u16,
    /// How many of the first values go to registers `0..in_registers` rather than to their
    /// parameters' locals: the parameters the compiler promoted in place, register `k` holding
    /// parameter `k` (`EnvironmentWindow::parameter_registers`, at most `bound`). A function that
    /// makes closures keeps those none of them names in registers (`CallPlan::environment_activation`).
    pub(crate) in_registers: u16,
    /// The rest parameter's local and the array the tier gathered for it.
    pub(crate) rest: Option<(u16, W)>,
    /// The self binding's local and the callee.
    pub(crate) self_binding: Option<(u16, W)>,
}

/// Enters an environment callee's window at `registers` (register 0), past its admission
/// (`admit_environment`): the composite row of the family.
///
/// Writes, in order: the `live` registers, the first `passed.in_registers` their arguments and the
/// others `undefined` (`protocol::fill_call`); the environment
/// entries (`lay_out_entries`); the `locals` `undefined`; the passed values as parameter locals
/// and kept values (`write_arguments`); the rest array's and the self binding's locals; the frame
/// context (`bind_context`). Reads only the passed values. Returns the address of the first
/// local. The tier then moves the values' top past `shape.held() + count` and the frame context,
/// and pushes the activation record (environment activations are never plain).
#[inline(always)]
pub fn enter_environment<S: Stage>(stage: &mut S, registers: S::Word, shape: EnvironmentShape, passed: EnvironmentPassed<S::Word>,
    context: WindowContext<S::Word>) -> S::Word {
    // The parameters promoted in place start as their arguments, every other live register
    // `undefined`; a promoted parameter's register past the live ones is dead at entry.
    super::protocol::fill_call(stage, registers, &[], Passed::Window { arguments: passed.arguments, count: passed.in_registers }, shape.live);
    let entries = stage.add_imm(registers, word_bytes(shape.registers));
    lay_out_entries(stage, entries, shape.captured, shape.locals);
    let local = stage.add_imm(entries, word_bytes(ENTRY_WORDS));
    super::protocol::fill_call(stage, local, &[], Passed::Window { arguments: local, count: 0 }, shape.locals);
    write_arguments(stage, local, shape.locals, (passed.arguments, passed.count), (passed.in_registers, passed.bound));
    let at = Region::Frame.mutable();
    if let Some((slot, gathered)) = passed.rest {
        stage.store_u64(at, local, words_at(u32::from(slot)), gathered);
    }
    if let Some((slot, callee)) = passed.self_binding {
        stage.store_u64(at, local, words_at(u32::from(slot)), callee);
    }
    bind_context(stage, registers, context);
    local
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;

    #[test]
    fn admission_is_the_record_limit_and_the_values_room() {
        assert!(concrete_admit_environment(3, 4, 10, 10));
        assert!(!concrete_admit_environment(4, 4, 10, 10), "the limit counts the records, the new one included");
        assert!(!concrete_admit_environment(3, 4, 9, 10));
    }

    #[test]
    fn an_environment_window_holds_registers_entries_locals_and_kept_values() {
        const CONTEXT: usize = frame::CONTEXT_WORDS;
        let undefined = Value::UNDEFINED.to_bits();
        let passed = [11_u64, 22, 33];
        let mut words = [0_u64; CONTEXT + 2 + ENTRY_WORDS as usize + 3 + 3];
        let registers = unsafe { words.as_mut_ptr().add(CONTEXT) } as u64;
        let shape = EnvironmentShape { registers: 2, live: 1, locals: 3, captured: 0b10 };
        let local = enter_environment(&mut Concrete::new(std::ptr::null()), registers, shape,
            EnvironmentPassed { arguments: passed.as_ptr() as u64, count: 3, bound: 2, in_registers: 0, rest: None, self_binding: Some((2, 99)) },
            WindowContext { this: 7, env: 8, capture: 9 });
        let entries = registers + 2 * 8;
        assert_eq!(local, entries + u64::from(ENTRY_WORDS) * 8);
        let w = &words[CONTEXT..];
        assert_eq!(w[0], undefined);
        assert_eq!(w[1], 0, "a register past the live ones is not written");
        assert_eq!(&w[2..7], &[Value::DELETED.to_bits(), entries + 24, 0b10, entries + 32, 3]);
        assert_eq!(&w[7..10], &[11, 22, 99], "parameters bound, the rest undefined, then the self binding");
        assert_eq!(&w[10..13], &passed, "every value kept after the locals");
        assert_eq!(&words[CONTEXT - 3..CONTEXT], &[9, 8, 7], "capture, env, this below register 0");
    }

    #[test]
    fn parameters_promoted_in_place_start_in_their_registers_and_leave_their_locals_undefined() {
        const CONTEXT: usize = frame::CONTEXT_WORDS;
        let undefined = Value::UNDEFINED.to_bits();
        let passed = [11_u64, 22, 33];
        let mut words = [0_u64; CONTEXT + 3 + ENTRY_WORDS as usize + 3 + 3];
        let registers = unsafe { words.as_mut_ptr().add(CONTEXT) } as u64;
        let shape = EnvironmentShape { registers: 3, live: 3, locals: 3, captured: 0b100 };
        let local = enter_environment(&mut Concrete::new(std::ptr::null()), registers, shape,
            EnvironmentPassed { arguments: passed.as_ptr() as u64, count: 3, bound: 3, in_registers: 2, rest: None, self_binding: None },
            WindowContext { this: 7, env: 8, capture: 9 });
        let w = &words[CONTEXT..];
        assert_eq!(&w[0..3], &[11, 22, undefined], "promoted parameters in registers 0 and 1, the rest `undefined`");
        let locals = (local - registers) as usize / 8;
        assert_eq!(&w[locals..locals + 3], &[undefined, undefined, 33], "a promoted parameter's local stays `undefined`");
        assert_eq!(&w[locals + 3..locals + 6], &passed, "every value kept after the locals");
    }
}
