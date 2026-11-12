//! The region's definition ids: every instruction owns two definition slots after the entry
//! definitions, its result's and its second register's (`second_definition`). A definition id
//! names the instruction it belongs to and which of its slots it is, so the web arena, the
//! verifier and the leaving definitions decode one id the same way.

use super::Definition;

/// The definitions an instruction owns: its result, then its second register.
pub(super) const SLOTS_PER_INSTRUCTION: usize = 2;

/// A definition id resolved: the instruction offset it belongs to, and whether it is that
/// instruction's second definition.
#[derive(Clone, Copy)]
pub(super) struct Slot {
    pub(super) offset: usize,
    pub(super) second: bool,
}

/// The definition id of the instruction at `offset`'s result.
pub(super) fn result(entry_count: usize, offset: usize) -> Definition {
    entry_count + SLOTS_PER_INSTRUCTION * offset
}

/// `definition` resolved to its instruction, or `None` for an entry definition.
pub(super) fn of(entry_count: usize, definition: Definition) -> Option<Slot> {
    let slot = definition.checked_sub(entry_count)?;
    Some(Slot {
        offset: slot / SLOTS_PER_INSTRUCTION,
        second: slot % SLOTS_PER_INSTRUCTION != 0,
    })
}
