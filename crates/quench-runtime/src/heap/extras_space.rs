//! The extras of live class cells, by id (`ObjectExtras`). A live cell's header high half names its entry here
//! (`ClassSpace::extras_id`); an ordinary object's extras move in at its birth (`Heap::insert_object_as_cell`), or the
//! entry is made on first need (`Heap::ensure_cell_extras`), and it is released when its cell dies (`ClassSpace::sweep_cells`).
//! Entry 0 is reserved as `NO_EXTRAS`, so a zero id names no extras. Records (arrays, functions, builtins) keep their own extras box until every
//! object's extras move here (C5 D4), so each object's extras have exactly one home at any time.

use super::cell::{Object, ObjectExtras, trace_extras};
use super::class_space::header_class;
use crate::value::Value;

/// The id of a cell with no extras: the reserved entry 0.
pub(crate) const NO_EXTRAS: u32 = 0;

/// A shared view of the cell extras for a parallel mark round (`ParallelMarkView`). `ObjectExtras` holds a watch's
/// pointer, so the extras are not `Sync` by their type; a round only reads them, and no worker writes an extras entry
/// while it runs (the cells' fields and the spill arena are read the same way).
#[derive(Clone, Copy)]
pub(crate) struct SharedExtras<'a>(pub(crate) &'a ExtrasSpace);

// SAFETY: see `SharedExtras`: the round's readers share the extras read-only, and the collector joins them before any
// write to the space.
unsafe impl Send for SharedExtras<'_> {}
// SAFETY: as `Send`.
unsafe impl Sync for SharedExtras<'_> {}

/// The slab of cell extras and its free list of released ids.
pub(crate) struct ExtrasSpace {
    entries: Vec<ObjectExtras>,
    free: Vec<u32>,
}

impl Default for ExtrasSpace {
    fn default() -> Self {
        Self { entries: vec![ObjectExtras::default()], free: Vec::new() }
    }
}

impl ExtrasSpace {
    /// Stores `extras` under a fresh id, reusing a released one first. The id is never `NO_EXTRAS`.
    pub(crate) fn alloc(&mut self, extras: ObjectExtras) -> u32 {
        match self.free.pop() {
            Some(id) => {
                self.entries[id as usize] = extras;
                id
            }
            None => {
                self.entries.push(extras);
                (self.entries.len() - 1) as u32
            }
        }
    }

    /// The extras of entry `id`; `None` for `NO_EXTRAS`.
    #[inline(always)]
    pub(crate) fn get(&self, id: u32) -> Option<&ObjectExtras> {
        (id != NO_EXTRAS).then(|| &self.entries[id as usize])
    }

    /// The extras of entry `id` exclusively; `None` for `NO_EXTRAS`.
    #[inline(always)]
    pub(crate) fn get_mut(&mut self, id: u32) -> Option<&mut ObjectExtras> {
        (id != NO_EXTRAS).then(|| &mut self.entries[id as usize])
    }

    /// Releases entry `id`, returning its extras (dropped by a dead cell's sweep, moved into a record by a departure).
    /// The id is free for reuse afterwards. The caller passes a live id, not `NO_EXTRAS`.
    pub(crate) fn release(&mut self, id: u32) -> ObjectExtras {
        debug_assert!(id != NO_EXTRAS, "the reserved entry holds no extras");
        let extras = std::mem::take(&mut self.entries[id as usize]);
        self.free.push(id);
        extras
    }

    /// Visits the values entry `id` holds, as a marked cell's extras are traced (`trace_extras`).
    pub(crate) fn trace(&self, id: u32, visit: impl FnMut(Value)) {
        if let Some(extras) = self.get(id) {
            trace_extras(extras, visit);
        }
    }

    /// The entries and the free list, for the snapshot (`heap::snapshot`).
    pub(crate) fn snapshot_parts(&self) -> (&[ObjectExtras], &[u32]) {
        (&self.entries, &self.free)
    }

    /// The space a snapshot restored: `entries` (entry 0 first) and `free`.
    pub(crate) fn from_snapshot(entries: Vec<ObjectExtras>, free: Vec<u32>) -> Self {
        Self { entries, free }
    }
}

impl super::Heap {
    /// The layout change of live cell `granule`, made before its class takes effect: a marked prototype's watchers
    /// retire (`ObjectExtras::note_prototype_layout_change`). A cell without extras has no mark, so nothing retires.
    pub(crate) fn note_cell_layout_change(&self, granule: u32) {
        let Some(extras) = self.extras.get(self.class_space.extras_id(granule)) else { return };
        let class = header_class(self.class_space.header(granule));
        extras.note_prototype_layout_change(Object::of_class(class).exact_shape());
    }

    /// Makes sure the live class cell `value` names has extras: an empty entry, on first need. A value that names no
    /// live cell (a record or a non-object) is left alone.
    pub(crate) fn ensure_cell_extras(&mut self, value: Value) {
        let Some(granule) = self.live_class_cell(value) else { return };
        if self.class_space.extras_id(granule) == NO_EXTRAS {
            let id = self.extras.alloc(ObjectExtras::default());
            self.class_space.set_extras_id(granule, id);
        }
    }
}
