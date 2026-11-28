//! The delete of a class cell's last-added key (departure-free S4): an in-place class transition back to the shape the
//! object had before that key's add. Any other delete makes the cell a dictionary cell in place instead
//! (`Heap::cell_to_dictionary`), so the cell stays a cell.

use super::Heap;
use super::class_space::header_class;
use super::layouts;
use crate::value::Value;

impl Heap {
    /// Drops the last-added field of live class cell `granule`, whose class moves to `parent`: the shape before that key's
    /// add, with `parent`'s fields. The vacated slot becomes `Value::DELETED` through the slot accessor, so a spilled slot
    /// clears its run word. Once the fields fit the region again, the run folds back into the cell (`ClassSpace::fold_run`).
    /// The fold moves fields, and a marked holder's vacated field is read by address as well, so the holder edge fires on
    /// every drop (`holder_fields_moved`).
    pub(crate) fn class_cell_drop_last(&mut self, granule: u32, parent: u32) {
        self.note_cell_layout_change(granule);
        let before = header_class(self.class_space.header(granule));
        debug_assert!(!layouts::class_is_dictionary(before), "a dictionary cell drops no key in place");
        let kept = self.properties.shape_length(parent);
        let from = self.class_spill_from(before);
        self.class_space.set_word(granule, from, kept, Value::DELETED);
        let region = layouts::class_region(before);
        if let (Some(spill), None) = (from, self.shape_spill_from(region, parent)) {
            self.class_space.fold_run(granule, spill);
        }
        let next = layouts::intern_class(parent, layouts::class_link(before), layouts::class_flags(before), region);
        self.class_space.write_class(granule, next);
        self.set_class(layouts::class_shape(before), parent);
        self.holder_fields_moved(granule);
    }
}
