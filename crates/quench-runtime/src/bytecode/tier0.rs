//! The first tier of a function's code: its first runs execute the record the load emitted,
//! unlowered, and a later call lowers it. Lowering costs about a thousand instructions per
//! bytecode instruction, interpreting one about fifty (octane-code-load lowering nothing:
//! -11.9% instructions). Only a loop-free body that never suspends runs unlowered, and it
//! lowers only at a call boundary with no frame of it live and before any lane view derives
//! from it (`Vm::promote_tier0_if_due`): frames and views name code by function index and pc.

use std::cell::Cell;
use std::rc::Rc;

use super::{Function, FunctionSlot, ProgramKind, RecordUse, RegisterRoots, ResidualProgram};

/// A function's unlowered record, and whether a lane view derived from it,
/// in any program sharing the record (`ResidualProgram::pin_tier0`).
#[derive(Clone, Debug)]
pub(crate) struct Tier0 {
    record: Rc<Function>,
    pinned: Cell<bool>,
}

impl Tier0 {
    #[inline]
    pub(super) fn record(&self) -> &Function {
        &self.record
    }

    fn into_record(self) -> Rc<Function> {
        self.record
    }
}

/// A slot's `Tier0`, set once like a `OnceCell`, which a full collection also empties once
/// the function has lowered and no frame runs the copy (`ResidualProgram::release_lowered_tier0`):
/// a lowered function kept its unlowered copy beside its record for good (jsdom-d3-startup:
/// 6K such copies).
#[derive(Default)]
pub(crate) struct Tier0Cell(std::cell::UnsafeCell<Option<Box<Tier0>>>);

impl Tier0Cell {
    #[inline(always)]
    pub(super) fn get(&self) -> Option<&Tier0> {
        // SAFETY: the cell is written only while empty (`get_or_init`), or emptied when no
        // reference from here is live (`release`).
        unsafe { (*self.0.get()).as_deref() }
    }

    pub(super) fn get_or_init(&self, init: impl FnOnce() -> Box<Tier0>) -> &Tier0 {
        if self.get().is_none() {
            let tier0 = init();
            // SAFETY: empty, so no reference from `get` exists; `init` does not reach the cell.
            unsafe { *self.0.get() = Some(tier0) };
        }
        self.get().expect("the cell was just set")
    }

    pub(super) fn take(&mut self) -> Option<Box<Tier0>> {
        self.0.get_mut().take()
    }

    /// Drops the copy.
    ///
    /// # Safety
    /// No reference `get` returned is live.
    unsafe fn release(&self) {
        // SAFETY: per the contract.
        drop(unsafe { (*self.0.get()).take() });
    }
}

impl Clone for Tier0Cell {
    fn clone(&self) -> Self {
        Self(std::cell::UnsafeCell::new(self.get().map(|tier0| Box::new(tier0.clone()))))
    }
}

impl std::fmt::Debug for Tier0Cell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(f)
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

/// Whether function `index`, unlowered, may run as it is.
fn runs_unlowered(program: &ResidualProgram, index: usize, function: &Function) -> bool {
    index != crate::vm::ROOT_FUNCTION_ID as usize
        && program.kind != ProgramKind::Wasm
        && !function.loops()
        && !function.suspends()
        // A program running from its kept form lowers every record on first read, as its
        // decoded ones are: a first-call chunk appended to it adds records that would otherwise
        // run unlowered, and a recursive one (a parser's descent, a tree walk's visitor) keeps
        // a frame of itself live through its runs until its unlowered record is pinned for good
        // (`ResidualProgram::pin_tier0`): acorn-wtb +8% per steady iteration.
        && program.encoded_records.is_none()
}

impl ResidualProgram {
    /// The record function `index`'s first runs execute: a copy of the load's unlowered
    /// record where it may run unlowered.
    pub(super) fn tier0_record(&self, index: usize) -> Option<&Function> {
        let slot = &self.functions[index];
        if let Some(tier0) = slot.tier0.get() {
            return Some(tier0.record());
        }
        if slot.header().is_async || slot.header().is_generator {
            return None;
        }
        // Copied before any run derives facts into it (its call plan, its decoded form): the
        // unlowered record stays as the load left it for the lowering.
        let mut function = slot.unlowered.with(|unlowered| unlowered.map(|unlowered| unlowered.function(slot.header())))?;
        if !runs_unlowered(self, index, &function) {
            // The lowering this read goes on to takes the record unpacked here, not again.
            slot.unlowered.set(Some(super::Unlowered::Record(Rc::new(function))));
            return None;
        }
        // Every register is a root and starts `undefined`: no liveness runs for it.
        function.register_roots = RegisterRoots::new(false);
        function.decoded.prime_loop(&function.code, &function.wide);
        let tier0 = slot.tier0.get_or_init(|| Box::new(Tier0 { record: Rc::new(function), pinned: Cell::new(false) }));
        Some(tier0.record())
    }

    /// Whether function `index` runs unlowered and unpinned, so it lowers now.
    pub(crate) fn tier0_due(&self, index: usize) -> bool {
        let slot = &self.functions[index];
        slot.record.get().is_none() && slot.tier0.get().is_some_and(|tier0| !tier0.pinned.get())
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

    /// Drops function `index`'s unlowered copy where it has lowered: its calls read the
    /// lowered record, and it lowered only with no frame of it live.
    ///
    /// # Safety
    /// Called at a full collection with no frame of any program sharing this residual running
    /// the function, and nothing else holding a reference into the copy.
    pub(crate) unsafe fn release_lowered_tier0(&self, index: usize) {
        if let Some(slot) = self.functions.get(index)
            && slot.record.get().is_some()
        {
            // SAFETY: the caller's; a lowered record answers every read of the slot.
            unsafe { slot.tier0.release() };
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

impl ResidualProgram {
    /// Function `index`'s lowered record, lowering it now where it would run unlowered: what an
    /// ahead compile reads, since a tier-0 record has no liveness (`RegisterRoots::new(false)`)
    /// and its PCs are not the lowered record's, which every later run executes.
    pub fn lowered_function(&self, index: usize) -> &Function {
        self.function(index);
        self.promote_tier0(index);
        self.function(index)
    }
}
