//! A `LoadCapture` of a root slot written once (`ProgramStore::root_slot_written_once`): once a
//! read found the slot initialized, the record keeps the value it found and every later read
//! is the record's constant, as C reads a constant address: dispatch, one load, one store.
//!
//! The record's operand holds the value (`LaneOperand::constant`) and its `b` and `c` the
//! capture's immediate, which no `LoadCapture` record reads otherwise. The value stays the
//! slot's: nothing else writes it, and the slot keeps it alive. Every collection returns each
//! such record to the capture's read (`Vm::retire_root_constants`), which keeps the value
//! again at its next run: no fact a collection could change outlives one.
use super::*;

script_handler! {
    /// A `LoadCapture` of a root slot written once, whose value the record keeps
    /// (`hold_root_constant`).
    pub(super) fn lane_load_root_constant<>(cx, r, ip, i, view) {
        r.set(i.a, i.constant());
        next!(cx, r, following(ip), view)
    }
}

/// Makes the record at `ip`, a plain `LoadCapture` whose capture walk just read `value` from
/// root `slot` of the root environment `depth` layers out, keep `value`
/// (`lane_load_root_constant`) where that environment is the running program's module
/// environment, the slot is written once and `value` is initialized; whether it did.
pub(super) fn hold_root_constant<H: Host>(cx: *mut LaneContext<H>, ip: Ip, depth: u16, slot: u16, value: Value) -> bool {
    if value.is_internal_marker() || !handler_is::<H>(ip, lane_load_capture_walk::<H>) {
        return false;
    }
    let vm = unsafe { vm(cx) };
    let program = unsafe { (*cx).program };
    let Some(root) = lane_root_environment(cx, depth) else { return false };
    if vm.programs.module_environment(program) != Some(root) || !vm.programs.root_slot_written_once(program, slot) {
        return false;
    }
    // SAFETY: `ip` is a record of the running function's view, whose first record is `base`.
    let pc = unsafe { ip.offset_from((*cx).base) };
    let Ok(pc) = u32::try_from(pc) else { return false };
    vm.programs.note_root_constant_site(program, lane_running_function(cx), pc);
    // SAFETY: as `requicken`: the record lives in an `UnsafeCell` of the running view, on the
    // view's thread, and no reference to its fields is live; it runs `lane_load_capture_walk`,
    // whose operand is `imm` and which reads neither `b` nor `c`.
    unsafe {
        let record = ip.cast_mut();
        let imm = (*record).operand.imm;
        (*record).b = imm as u16;
        (*record).c = (imm >> u16::BITS) as u16;
        (*record).operand.constant = value;
    }
    requicken::<H>(ip, lane_load_root_constant::<H>);
    true
}

impl LaneView {
    /// Returns the record at `pc`, where it keeps a root slot's value
    /// (`lane_load_root_constant`), to the capture's read: its immediate back in its operand.
    /// A record that no longer keeps one (its view was derived again) stays as it is.
    fn release_root_constant<H: Host>(&self, pc: u32) {
        let Some(cell) = self.records.get(pc as usize) else { return };
        let record = std::cell::UnsafeCell::raw_get(cell);
        if !handler_is::<H>(record.cast_const(), lane_load_root_constant::<H>) {
            return;
        }
        // SAFETY: no lane handler runs during a collection, and the record's `b` and `c` hold
        // the immediate `hold_root_constant` moved there.
        unsafe {
            (*record).operand = LaneOperand { imm: u32::from((*record).b) | (u32::from((*record).c) << u16::BITS) };
            (*record).b = 0;
            (*record).c = 0;
            (*record).handler = lane_load_capture_walk::<H> as Handler as *const ();
        }
    }
}

impl<H: Host> Vm<H> {
    /// Returns every record that keeps a root slot's value to the capture's read
    /// (`LaneView::release_root_constant`): a collection ran.
    pub(in crate::vm) fn retire_root_constants(&self) {
        self.programs.retire_root_constant_sites(|view, pc| view.release_root_constant::<H>(pc));
    }
}
