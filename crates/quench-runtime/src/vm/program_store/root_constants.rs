//! A program's root binding slots written once (`ResidualProgram::root_slots_written_once`),
//! derived on first read, and the runs of its root. A written-once slot a read found
//! initialized holds that value for every later read, while the root has run at most once,
//! so its root environment is the program's only one.
use super::{ProgramId, ProgramStore};

#[derive(Default)]
pub(super) struct RootConstants {
    /// The membership words of the written-once slots (`BitSet::words`), derived on first read,
    /// and the functions that derivation judged by their captures, their records unbuilt
    /// (`RootWrittenOnce::unbuilt`): a read of a slot outside them derives them again once one
    /// of those records is built, as a lazily compiled program builds its functions' records
    /// on their first calls, after another site's read derived the slots.
    written_once: std::cell::RefCell<Option<(Box<[u64]>, Box<[u32]>)>>,
    /// The root closures made for the program (`Vm::note_root_run`), counted up to two: past
    /// one, no root slot is written-once for reads anymore.
    root_runs: std::cell::Cell<u8>,
}

/// The root runs past which a program has more than one root environment.
const ONE_ROOT_RUN: u8 = 1;

impl ProgramStore {
    /// Whether root `slot` of program `id` holds the binding itself and is written once, while
    /// the program's root has run at most once: a value a read found initialized there is then
    /// every later read's.
    #[allow(dead_code)]
    pub(crate) fn root_slot_written_once(&self, id: ProgramId, slot: u16) -> bool {
        let Some(entry) = self.entry(id) else { return false };
        if entry.root_constants.root_runs.get() > ONE_ROOT_RUN
            || self.root_slot(id, slot) != crate::bytecode::RootSlotRule::Binding
        {
            return false;
        }
        let mut held = entry.root_constants.written_once.borrow_mut();
        let stale = held.as_ref().is_none_or(|(words, unbuilt)| {
            !crate::bitset::words_contain(words, usize::from(slot)) && entry.residual.any_record_built(unbuilt)
        });
        if stale {
            let derived = entry.residual.root_slots_written_once();
            *held = Some((derived.once.words().into(), derived.unbuilt));
        }
        held.as_ref().is_some_and(|(words, _)| crate::bitset::words_contain(words, usize::from(slot)))
    }

    /// Notes another run of program `id`'s root (its root closure is made).
    pub(crate) fn note_root_run(&self, id: ProgramId) {
        let Some(entry) = self.entry(id) else { return };
        let runs = &entry.root_constants.root_runs;
        runs.set(runs.get().saturating_add(1).min(ONE_ROOT_RUN + 1));
    }
}
