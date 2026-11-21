//! A `LoadCapture` of a root slot written once (`ProgramStore::root_slot_written_once`): once a
//! read found the slot initialized, the record keeps the value it found and every later read
//! is the record's constant, as C reads a constant address: dispatch, one load, one store.
//!
//! The record's operand holds the value (`LaneOperand::constant`) and its `b` and `c` the
//! capture's immediate, which no `LoadCapture` record reads otherwise. The value stays the
//! slot's while the program's root has run once (one root environment): nothing else writes
//! the slot, and the slot keeps it alive. A second run of the root returns every record of
//! the program to the capture's read for good (`Vm::note_root_run`); so does each collection,
//! after which a record keeps the value again at its next run.
use super::*;

script_handler! {
    /// A `LoadCapture` of a root slot written once, whose value the record keeps
    /// (`hold_root_constant`).
    pub(super) fn lane_load_root_constant<>(cx, r, ip, i, view) {
        r.set(i.a, i.constant());
        next!(cx, r, following(ip), view)
    }
}

/// Whether the record at `ip` may keep a value in its operand: a capture read whose operand is
/// its immediate alone, and no pair's second record, whose immediate the pair's first reads
/// (`lane_load_outer_capture_pair`). A record a loop header or kernel holds runs neither.
fn holds_plain_capture_read(cx: *mut LaneContext, ip: Ip) -> bool {
    if handler_is(ip, lane_load_outer_capture) {
        // SAFETY: a record past the view's first has one before it in the same view.
        return ip <= unsafe { (*cx).base } || !handler_is(unsafe { ip.sub(1) }, lane_load_outer_capture_pair);
    }
    handler_is(ip, lane_load_capture)
        || handler_is(ip, lane_load_capture_walk)
        || handler_is(ip, lane_load_root_binding)
}

/// Makes the record at `ip`, a capture read whose walk just read `value` from root `slot` of
/// `owner`'s root environment (`lane_root_binding_owner`), keep `value`
/// (`lane_load_root_constant`) where `owner` is the running program, the slot is written once
/// and `value` is initialized; whether it did.
pub(super) fn hold_root_constant(cx: *mut LaneContext, ip: Ip, owner: ProgramId, slot: u16, value: Value) -> bool {
    let vm = unsafe { vm(cx) };
    let program = unsafe { (*cx).program };
    if value.is_internal_marker() || owner != program || !holds_plain_capture_read(cx, ip) || !vm.programs.root_slot_written_once(program, slot) {
        return false;
    }
    // SAFETY: `ip` is a record of the running function's view, whose first record is `base`.
    let Ok(pc) = u32::try_from(unsafe { ip.offset_from((*cx).base) }) else { return false };
    vm.programs.note_root_constant_site(program, lane_running_function(cx), pc);
    // SAFETY: as `requicken`: the record lives in an `UnsafeCell` of the running view, on the
    // view's thread, and no reference to its fields is live; its operand is `imm`
    // (`holds_plain_capture_read`), and no capture read reads its `b` or `c`.
    unsafe {
        let record = ip.cast_mut();
        let imm = (*record).operand.imm;
        (*record).b = imm as u16;
        (*record).c = (imm >> u16::BITS) as u16;
        (*record).operand.constant = value;
    }
    requicken(ip, lane_load_root_constant);
    true
}

impl LaneView {
    /// Returns the record at `pc`, where it keeps a root slot's value
    /// (`lane_load_root_constant`), to the capture's read (`lane_load_capture_walk`), its
    /// immediate back in its operand. A record that no longer keeps one (its view was derived
    /// again) stays as it is.
    fn release_root_constant(&self, pc: u32) {
        let Some(cell) = self.records.get(pc as usize) else { return };
        let record = std::cell::UnsafeCell::raw_get(cell);
        if !handler_is(record.cast_const(), lane_load_root_constant) {
            return;
        }
        // SAFETY: no lane handler reads the record meanwhile (a collection, or a root's start,
        // runs outside its handler), and its `b` and `c` hold the immediate
        // `hold_root_constant` moved there.
        unsafe {
            (*record).operand = LaneOperand { imm: u32::from((*record).b) | (u32::from((*record).c) << u16::BITS) };
            (*record).b = 0;
            (*record).c = 0;
            (*record).handler = lane_load_capture_walk as Handler as *const ();
        }
    }
}

impl Vm {
    /// Returns every record that keeps a root slot's value to the capture's read
    /// (`LaneView::release_root_constant`): a collection ran.
    pub(in crate::vm) fn retire_root_constants(&self) {
        self.programs.retire_root_constant_sites(None, |view, pc| view.release_root_constant(pc));
    }

    /// Notes that the active program's root starts another run (its root closure is made):
    /// from its second, its root environment is not the only one, so its records that keep
    /// a root slot's value read the slot again, and none keeps one again.
    pub(in crate::vm) fn note_root_run(&self) {
        let program = self.active_program;
        if self.programs.note_root_run(program) {
            self.programs.retire_root_constant_sites(Some(program), |view, pc| view.release_root_constant(pc));
        }
    }
}
