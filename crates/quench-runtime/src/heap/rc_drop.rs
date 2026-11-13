//! Dead cells a sweep worker hands to the collector thread, and the lock stripes that
//! serialize the release of reference-counted payloads two cells may share.

use super::Cell;
use std::rc::Rc;
use std::sync::Mutex;

const RC_DROP_LOCK_STRIPES: usize = 64;

static SHARED_RC_DROP_LOCK: Mutex<()> = Mutex::new(());
static RC_DROP_LOCKS: [Mutex<()>; RC_DROP_LOCK_STRIPES] =
    [const { Mutex::new(()) }; RC_DROP_LOCK_STRIPES];

/// A dead cell moved from a sweep worker to the collector thread. Rc-bearing
/// cells are released by that owner after the sweep batch joins; the
/// destructor's lock is a safe fallback if a transfer cannot be completed.
pub(super) struct StoppedCellDrop(pub(super) Option<Cell>);

// SAFETY: a sweep worker only moves the detached Cell into this wrapper and
// reads an Rc allocation pointer to classify it. It does not clone, inspect or
// drop any Rc payload. The collector thread drops transferred cells after the
// sweep batch joins; the fallback Drop path serializes worker-side destruction.
unsafe impl Send for StoppedCellDrop {}

impl StoppedCellDrop {
    pub(super) fn drop_on_owner(mut self) {
        if let Some(cell) = self.0.take() {
            drop(cell);
        }
    }
}

impl Drop for StoppedCellDrop {
    fn drop(&mut self) {
        let Some(cell) = self.0.take() else {
            return;
        };
        let Some(lock) = cell_rc_drop_lock(&cell) else {
            drop(cell);
            return;
        };

        let guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cell_has_shared_rc_payload(&cell) {
            drop(cell);
            drop(guard);
        } else {
            drop(guard);
            drop(cell);
        }
    }
}

pub(super) fn cell_rc_drop_lock(cell: &Cell) -> Option<&'static Mutex<()>> {
    match cell {
        Cell::Array { object } => {
            Some(&RC_DROP_LOCKS[rc_drop_lock_index(object.elements().as_ptr() as usize)])
        }
        Cell::ArrayBuffer { bytes, .. } => Some(if bytes.holds_external_rc() {
            &SHARED_RC_DROP_LOCK
        } else {
            &RC_DROP_LOCKS[rc_drop_lock_index(bytes.as_ptr() as usize)]
        }),
        Cell::WasmHostFunction { signature, .. } => {
            Some(&RC_DROP_LOCKS[rc_drop_lock_index(Rc::as_ptr(signature) as usize)])
        }
        Cell::Iterator { .. }
        | Cell::WasmGc { .. }
        | Cell::WasmGlobal { .. }
        | Cell::WasmTable { .. }
        | Cell::WasmTag { .. }
        | Cell::RegExp { .. } => Some(&SHARED_RC_DROP_LOCK),
        _ => None,
    }
}

fn cell_has_shared_rc_payload(cell: &Cell) -> bool {
    match cell {
        Cell::Array { object } => object.elements().owner_count() > 1,
        Cell::ArrayBuffer { bytes, .. } => bytes.is_shared(),
        Cell::WasmHostFunction { signature, .. } => Rc::strong_count(signature) > 1,
        Cell::Iterator { .. }
        | Cell::WasmGc { .. }
        | Cell::WasmGlobal { .. }
        | Cell::WasmTable { .. }
        | Cell::WasmTag { .. }
        | Cell::RegExp { .. } => true,
        _ => false,
    }
}

fn rc_drop_lock_index(address: usize) -> usize {
    (address >> 4) & (RC_DROP_LOCK_STRIPES - 1)
}
