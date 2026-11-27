//! The departure edge (R4-C4): a class cell gives up its class and becomes the record of its object.
//!
//! The object's `Value` never changes. The cell is its own stub: its header becomes
//! `departed_header(record)` and the new record's `stub` is the cell's granule, so every reader that
//! finds `DEPARTED_CLASS` takes the record hop (`ClassSpace::record_of`). The fields the object held
//! are copied into an arena run for the record; the cell keeps its field words, which the sweep frees
//! with the object, and its slack is `Value::DELETED`.
//!
//! A departed object takes its dictionary class from the VM (`Vm::mark_object_dictionary`), because a
//! dictionary shape lives in the VM's shape table. The record's class is the cell's class until then.

use super::cell::Object;
use super::class_space::{header_class, header_record, is_cell_granule, live_header, region_capacity};
use super::{Heap, OrdinaryRecord, layouts};
use crate::staged::object_encoding::{DEPARTED_CLASS, departed_header};
use crate::value::Value;
use crate::value_vec::INLINE_PROPERTY_COUNT;

impl Heap {
    /// The granule of `value` when it names a live class cell: one in a cell region whose header
    /// still holds its class. A record's object, and a departed cell, are not live cells.
    #[inline(always)]
    pub(crate) fn live_class_cell(&self, value: Value) -> Option<u32> {
        let granule = value.direct_object_index()?.granule();
        if !is_cell_granule(granule) {
            return None;
        }
        (header_class(self.class_space.header(granule)) != DEPARTED_CLASS).then_some(granule)
    }

    /// The departure edge: the ordinary object `value` names becomes a record, if it is a live class
    /// cell. Returns the record id that names the object from now on; a record's or a departed
    /// cell's id is returned unchanged. `None` for a value that is no ordinary object.
    pub(crate) fn depart_class_object(&mut self, value: Value) -> Option<u32> {
        let granule = value.direct_object_index()?.granule();
        let header = self.class_space.header(granule);
        if header_class(header) == DEPARTED_CLASS {
            return Some(header_record(header));
        }
        let class = header_class(header);
        let shape = layouts::class_shape(class);
        let length = self.properties.shape_length(shape);
        debug_assert!(length <= region_capacity(granule) as usize, "a class cell holds its class's fields");
        let fields: Vec<Value> = (0..length).map(|slot| self.class_space.field(granule, slot)).collect();
        let properties = self.properties.with_values(shape, &fields);
        let mut object = Object::with_property_storage(
            layouts::class_link(class).prototype(),
            properties,
            [Value::UNDEFINED; INLINE_PROPERTY_COUNT],
        );
        // The class's flags and prototype link; the storage form is the record's own.
        object.set_class(class);
        self.admit_object_contents(&object);
        let Some(record_id) = self.object_records.bump_id(&mut self.allocated) else {
            panic!("object records exhausted: the object space holds 2^30 records");
        };
        // SAFETY: the id is claimed uninitialized and written whole here, before any safepoint.
        unsafe { self.object_records.record_ptr(record_id).write(OrdinaryRecord { object, stub: granule }) };
        self.class_space.write_header(granule, departed_header(record_id));
        self.class_space.delete_slack(granule, length);
        self.properties.note_class_departure(shape);
        Some(record_id)
    }

    /// The departure edge for a record-only view of `value` (`object_mut_departing`, `object_and_arena_mut`,
    /// `object_record_mut`): a live class cell departs first, so the record the view names is the whole
    /// object. A record or a departed cell is unchanged, and a value that is no ordinary object is ignored.
    #[inline(always)]
    pub(crate) fn depart_for_mutation(&mut self, value: Value) {
        if value.direct_object_index().is_some_and(|object| is_cell_granule(object.granule())) {
            self.depart_class_object(value);
        }
    }

    /// Notes a write to live cell `granule` (`Object::note_constant_write`): a constant mark clears in
    /// place, its class word takes the cleared flags, and the readers of the mark retire.
    #[inline(always)]
    fn note_cell_write(&mut self, granule: u32) {
        let class = header_class(self.class_space.header(granule));
        let mut object = Object::of_class(class);
        object.note_constant_write();
        if object.layout_id() != class {
            self.class_space.write_header(granule, live_header(object.layout_id()));
        }
    }

    /// A store of `value` to field `slot` of live cell `granule`, in place (`Heap::property_set`): the
    /// store gate's barrier for a heap reference, the constant mark's clear, then the field.
    pub(crate) fn class_cell_store(&mut self, granule: u32, slot: usize, value: Value) {
        if value.heap_index().is_some() {
            self.class_space.barrier(granule, &self.properties);
        }
        self.note_cell_write(granule);
        self.class_space.set_field(granule, slot, value);
    }

    /// The add of `value` as field `slot` along the class edge from class `before`'s shape to shape `after`,
    /// in place for live cell `granule` where the field fits the cell's capacity (`region_capacity`). The cell
    /// keeps its link and flags, takes the after-shape's class, and the readers of the old shape retire
    /// (`Heap::set_class`). `false`, with nothing changed, where the field does not fit: the caller departs.
    pub(crate) fn class_cell_add(&mut self, granule: u32, before: u32, after: u32, slot: usize, value: Value) -> bool {
        if slot >= region_capacity(granule) as usize {
            return false;
        }
        self.class_cell_store(granule, slot, value);
        let class = header_class(self.class_space.header(granule));
        debug_assert_eq!(layouts::class_shape(class), before, "an add's cell holds its transition's before-shape");
        let next = layouts::intern_class(after, layouts::class_link(class), layouts::class_flags(class));
        self.class_space.write_header(granule, live_header(next));
        self.set_class(before, after);
        true
    }
}
