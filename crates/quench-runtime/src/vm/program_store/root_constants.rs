//! A program's root binding slots written once (`ResidualProgram::root_slots_written_once`),
//! derived on first read, the runs of its root, and the code that folded such a slot's value. A written-once slot a read found initialized holds that
//! value for every later read, while the root has run at most once, so its root environment
//! is the program's only one.
use super::{ProgramId, ProgramStore};
use crate::Value;
use crate::code_dependencies::{CodeReaders, DependentCode};
use std::rc::Rc;

#[derive(Default)]
pub(super) struct RootConstants {
    /// The membership words of the written-once slots (`BitSet::words`), derived on first read,
    /// and the functions that derivation judged by their captures, their records unbuilt
    /// (`RootWrittenOnce::unbuilt`): a read of a slot outside them derives them again once one
    /// of those records is built, as a lazily compiled program builds its functions' records
    /// on their first calls, after another site's read derived the slots.
    written_once: std::cell::RefCell<Option<(Box<[u64]>, Box<[u32]>)>>,
    /// The root closures made for the program (`Vm::note_root_run`), counted up to two: past
    /// one, no root slot is written-once for reads anymore and no record keeps its value.
    root_runs: std::cell::Cell<u8>,
    /// The published code that folded a written-once slot's value (`Identity::Folded`), retired
    /// by the program's second root run (`ProgramStore::note_root_run`).
    readers: std::cell::RefCell<CodeReaders>,
    /// The environment of the root's first activation that captured its locals
    /// (`ProgramStore::note_root_environment`): the one a written-once slot is read from, while
    /// the root has run at most once. A GC root (`ProgramStore::extend_roots`).
    pub(super) environment: std::cell::Cell<Option<Value>>,
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

    /// Notes another run of program `id`'s root (its root closure is made); whether it is its
    /// second. From its second, the code that folded a written-once root slot is retired and
    /// its readers retire: its root
    /// environment is no longer the program's only one.
    pub(crate) fn note_root_run(&self, id: ProgramId) -> bool {
        let Some(entry) = self.entry(id) else { return false };
        let runs = &entry.root_constants.root_runs;
        runs.set(runs.get().saturating_add(1).min(ONE_ROOT_RUN + 1));
        if runs.get() > ONE_ROOT_RUN {
            entry.root_constants.readers.borrow_mut().retire(0);
            return true;
        }
        false
    }

    /// Makes `code`, which folded a written-once root slot of program `id`, retire at the root's
    /// second run (`note_root_run`); `false` where a reader table is full.
    pub(crate) fn subscribe_root_constant(&self, id: ProgramId, code: &Rc<dyn DependentCode>) -> bool {
        let Some(entry) = self.entry(id) else { return false };
        entry.root_constants.readers.borrow_mut().subscribe(0, code)
    }

    /// `subscribe_root_constant` for each of `programs`, the programs whose root constants a body
    /// folds (`LaneDependencies::root_programs`); whether all subscribed.
    pub fn subscribe_root_constants(&self, programs: &[ProgramId], code: &Rc<dyn DependentCode>) -> bool {
        programs.iter().all(|program| self.subscribe_root_constant(*program, code))
    }

    /// Records `environment`, the environment a root activation of program `id` promoted its
    /// locals to, where none is recorded yet: a written-once slot is read from it at compile time
    /// (`root_environment`). Its entry is marked traced anew (`entry_mut`), so a minor collection
    /// that finds it young keeps it.
    pub(crate) fn note_root_environment(&mut self, id: ProgramId, environment: Value) {
        if self.entry(id).is_none_or(|entry| entry.root_constants.environment.get().is_some()) { return; }
        if let Some(entry) = self.entry_mut(id) {
            entry.root_constants.environment.set(Some(environment));
        }
    }

    /// The root environment of program `id`: its module environment, or the one its root
    /// activation recorded (`note_root_environment`).
    pub fn root_environment(&self, id: ProgramId) -> Option<Value> {
        self.module_environment(id).or_else(|| self.entry(id)?.root_constants.environment.get())
    }
}
