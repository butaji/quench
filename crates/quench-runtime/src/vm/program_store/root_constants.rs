//! A program's root binding slots written once (`ResidualProgram::root_slots_written_once`),
//! derived on first read, and the read records that keep such a slot's value
//! (`lane_load_root_constant`), which every collection returns to the slot's read.
use super::{LaneViewCell, ProgramId, ProgramStore, derived_lane_view};

#[derive(Default)]
pub(super) struct RootConstants {
    /// The membership words of the written-once slots (`BitSet::words`), derived on first read.
    written_once: std::cell::OnceCell<Box<[u64]>>,
    /// The (function, pc) of each record that keeps a slot's value since the last collection.
    sites: std::cell::RefCell<Vec<(u32, u32)>>,
}

impl ProgramStore {
    /// Whether root `slot` of program `id` is written once and holds the binding itself, in a
    /// root body that runs once: a module's, whose root environment is the program's only
    /// one. A value a read found initialized there is every later read's.
    pub(crate) fn root_slot_written_once(&self, id: ProgramId, slot: u16) -> bool {
        let Some(entry) = self.entry(id) else { return false };
        if !entry.module || self.root_slot(id, slot) != crate::bytecode::RootSlotRule::Binding {
            return false;
        }
        let words = entry.root_constants.written_once.get_or_init(|| entry.residual.root_slots_written_once().words().into());
        crate::bitset::words_contain(words, usize::from(slot))
    }

    /// Notes that the record at `pc` of function `function`'s view of program `id` keeps a
    /// root slot's value (`lane_load_root_constant`).
    pub(crate) fn note_root_constant_site(&self, id: ProgramId, function: u32, pc: u32) {
        if let Some(entry) = self.entry(id) {
            entry.root_constants.sites.borrow_mut().push((function, pc));
        }
    }

    /// Hands each record that keeps a root slot's value to `revert`, with its view, and
    /// forgets it: a collection ran, after which each reads its slot again once.
    pub(crate) fn retire_root_constant_sites(&self, mut revert: impl FnMut(&super::super::dispatch_fast::LaneView, u32)) {
        for entry in self.programs.iter().flatten() {
            let sites = std::mem::take(&mut *entry.root_constants.sites.borrow_mut());
            let Some(views) = entry.lane_views.table.get().map(|table| &table[..]) else { continue };
            for (function, pc) in sites {
                if let Some(view) = views.get(function as usize).and_then(|cell: &LaneViewCell| derived_lane_view(cell)) {
                    revert(view, pc);
                }
            }
        }
    }
}
