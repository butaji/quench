//! The first tier of a function's code: its first runs execute the record the load emitted,
//! unlowered, and a later call lowers it. Lowering costs about a thousand instructions per
//! bytecode instruction, interpreting one about fifty (octane-code-load lowering nothing:
//! -11.9% instructions). Only a loop-free body that never suspends runs unlowered, and it
//! lowers only at a call boundary with no frame of it live and before any lane view derives
//! from it (`Vm::promote_tier0_if_due`): frames and views name code by function index and pc.

use std::cell::Cell;
use std::rc::Rc;

use super::{Function, FunctionSlot, ProgramKind, RecordUse, RegisterRoots, ResidualProgram};

/// A function's unlowered record, the runs it made, and whether a lane view derived from it,
/// in any program sharing the record (`ResidualProgram::pin_tier0`).
#[derive(Clone, Debug)]
pub(crate) struct Tier0 {
    record: Rc<Function>,
    runs: Cell<u8>,
    pinned: Cell<bool>,
}

impl Tier0 {
    pub(super) fn record(&self) -> &Function {
        &self.record
    }

    fn into_record(self) -> Rc<Function> {
        self.record
    }
}

impl FunctionSlot {
    /// A change to a record still running unlowered makes it the compiled one.
    pub(super) fn settle_tier0(&mut self) {
        if self.record.get().is_none() && let Some(tier0) = self.tier0.take() {
            let _ = self.record.set(tier0.into_record());
        }
    }
}

/// Whether function `index`, unlowered, may run as it is: not under `tier0-off`.
fn runs_unlowered(program: &ResidualProgram, index: usize, function: &Function) -> bool {
    index != crate::vm::ROOT_FUNCTION_ID as usize
        && program.kind != ProgramKind::Wasm
        && !crate::trace::Dev::Tier0Off.on()
        && !function.loops()
        && !function.suspends()
}

impl ResidualProgram {
    /// The record function `index`'s first runs execute: a copy of the load's unlowered
    /// record where it may run unlowered.
    pub(super) fn tier0_record(&self, index: usize) -> Option<&Function> {
        let slot = &self.functions[index];
        if let Some(tier0) = slot.tier0.get() {
            return Some(tier0.record());
        }
        if slot.header.is_async || slot.header.is_generator {
            return None;
        }
        // Copied before any run derives facts into it (its call plan, its decoded form): the
        // unlowered record stays as the load left it for the lowering.
        let mut function = slot.unlowered.borrow().as_ref().map(super::Unlowered::function)?;
        if !runs_unlowered(self, index, &function) {
            // The lowering this read goes on to takes the record unpacked here, not again.
            *slot.unlowered.borrow_mut() = Some(super::Unlowered::Record(Rc::new(function)));
            return None;
        }
        // Every register is a root and starts `undefined`: no liveness runs for it.
        function.register_roots = RegisterRoots::new(false);
        function.decoded.prime_loop(&function.code, &function.wide);
        let tier0 = slot.tier0.get_or_init(|| Box::new(Tier0 { record: Rc::new(function), runs: Cell::new(1), pinned: Cell::new(false) }));
        Some(tier0.record())
    }

    /// Whether function `index` runs unlowered, unpinned, and made its `runs`, counting the
    /// one asked for.
    pub(crate) fn tier0_due(&self, index: usize, runs: u8) -> bool {
        let slot = &self.functions[index];
        if slot.record.get().is_some() {
            return false;
        }
        let Some(tier0) = slot.tier0.get().filter(|tier0| !tier0.pinned.get()) else { return false };
        let made = tier0.runs.get();
        if made >= runs {
            return true;
        }
        tier0.runs.set(made + 1);
        false
    }

    /// Whether function `index` still runs its unlowered record, from which no lane view
    /// derives where the derivation may wait (`ProgramStore::entry_lane_view`).
    pub(crate) fn runs_tier0(&self, index: usize) -> bool {
        let slot = &self.functions[index];
        slot.record.get().is_none() && slot.tier0.get().is_some()
    }

    /// Keeps function `index` unlowered for good where it runs unlowered: a lane view of some
    /// program sharing this record derived from it, and views name code by pc.
    pub(crate) fn pin_tier0(&self, index: usize) {
        if let Some(tier0) = self.functions[index].tier0.get().filter(|_| self.runs_tier0(index)) {
            tier0.pinned.set(true);
        }
    }

    /// Lowers function `index` from the load's unlowered record. Its frames, none live,
    /// read the lowered record from now on; the unlowered copy stays with the program, as a
    /// record handed out earlier may still be read.
    pub(crate) fn promote_tier0(&self, index: usize) {
        let slot = &self.functions[index];
        if slot.tier0.get().is_some() {
            slot.record.get_or_init(|| Rc::new(self.built_record(index, RecordUse::Kept)));
        }
    }
}

#[cfg(test)]
impl ResidualProgram {
    /// Function `index`'s lowered record, lowering it now where it would run unlowered.
    pub(crate) fn lowered_function(&self, index: usize) -> &Function {
        self.function(index);
        self.promote_tier0(index);
        self.function(index)
    }
}
