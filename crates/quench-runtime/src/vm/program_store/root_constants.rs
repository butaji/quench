//! A program's root binding slots written once (`ResidualProgram::root_slots_written_once`),
//! derived on first read, the runs of its root, and the read records that keep such a slot's
//! value (`lane_load_root_constant`): valid while the root has run at most once, so its root
//! environment is the program's only one, and until the next collection.
use super::{LaneViewCell, ProgramId, ProgramStore, derived_lane_view};

#[derive(Default)]
pub(super) struct RootConstants {
    /// The membership words of the written-once slots (`BitSet::words`), derived on first read.
    written_once: std::cell::OnceCell<Box<[u64]>>,
    /// The root closures made for the program (`Vm::note_root_run`), counted up to two: past
    /// one, no record keeps a root slot's value.
    root_runs: std::cell::Cell<u8>,
    /// The (function, pc) of each record that keeps a slot's value since the last collection.
    sites: std::cell::RefCell<Vec<(u32, u32)>>,
}

/// The root runs past which a program has more than one root environment.
const ONE_ROOT_RUN: u8 = 1;

impl ProgramStore {
    /// Whether root `slot` of program `id` holds the binding itself and is written once, while
    /// the program's root has run at most once: a value a read found initialized there is then
    /// every later read's.
    pub(crate) fn root_slot_written_once(&self, id: ProgramId, slot: u16) -> bool {
        let Some(entry) = self.entry(id) else { return false };
        if entry.root_constants.root_runs.get() > ONE_ROOT_RUN
            || self.root_slot(id, slot) != crate::bytecode::RootSlotRule::Binding
        {
            return false;
        }
        let words = entry.root_constants.written_once.get_or_init(|| entry.residual.root_slots_written_once().words().into());
        crate::bitset::words_contain(words, usize::from(slot))
    }

    /// Notes another run of program `id`'s root; whether it is its second, from which its
    /// records keep no root slot's value (`retire_root_constant_sites`).
    pub(crate) fn note_root_run(&self, id: ProgramId) -> bool {
        let Some(entry) = self.entry(id) else { return false };
        let runs = &entry.root_constants.root_runs;
        runs.set(runs.get().saturating_add(1).min(ONE_ROOT_RUN + 1));
        runs.get() > ONE_ROOT_RUN
    }

    /// Notes that the record at `pc` of function `function`'s view of program `id` keeps a
    /// root slot's value (`lane_load_root_constant`).
    pub(crate) fn note_root_constant_site(&self, id: ProgramId, function: u32, pc: u32) {
        if let Some(entry) = self.entry(id) {
            entry.root_constants.sites.borrow_mut().push((function, pc));
        }
    }

    /// Hands each record that keeps a root slot's value, of program `only` or of every
    /// program, to `revert` with its view, and forgets it.
    pub(crate) fn retire_root_constant_sites(
        &self,
        only: Option<ProgramId>,
        mut revert: impl FnMut(&super::super::dispatch_fast::LaneView, u32),
    ) {
        match only {
            Some(id) => self.entry(id).into_iter().for_each(|entry| retire_entry_sites(entry, &mut revert)),
            None => self.programs.iter().flatten().for_each(|entry| retire_entry_sites(entry, &mut revert)),
        }
    }
}

/// `retire_root_constant_sites` of one program.
fn retire_entry_sites(entry: &super::ProgramEntry, revert: &mut impl FnMut(&super::super::dispatch_fast::LaneView, u32)) {
    let sites = std::mem::take(&mut *entry.root_constants.sites.borrow_mut());
    let Some(views) = entry.lane_views.table.get().map(|table| &table[..]) else { return };
    for (function, pc) in sites {
        if let Some(view) = views.get(function as usize).and_then(|cell: &LaneViewCell| derived_lane_view(cell)) {
            revert(view, pc);
        }
    }
}
