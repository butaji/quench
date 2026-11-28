//! The departure edge (R4-C4): a class cell gives up its class and becomes the record of its object.
//!
//! The object's `Value` never changes. The cell is its own stub: its header becomes
//! `departed_header(record)` and the new record's `stub` is the cell's granule, so every reader that
//! finds `DEPARTED_CLASS` takes the record hop (`ClassSpace::record_of`). The fields the object held
//! are copied into an arena run for the record; the cell keeps its inline field words, which the sweep frees
//! with the object, and its slack is `Value::DELETED`.
//!
//! Departure is the final form only for extras and record-only views. A class whose shape outgrows its cell spills
//! instead (`heap::spill`), a dictionary-mode cell keeps its class and takes its fields in a table
//! (`heap::dictionary_cell`), and a delete is a class transition (`heap::class_delete`), so none of those departs.
//!
//! A departed object takes its dictionary class from the VM (`Vm::mark_object_dictionary`), because a
//! dictionary shape lives in the VM's shape table. The record's class is the cell's class until then.

use super::cell::Object;
use super::extras_space::NO_EXTRAS;
use super::class_space::{header_class, header_record, is_cell_granule, region_capacity};
use super::{Heap, OrdinaryRecord, layouts};
use crate::staged::field_cache_layout::CLASS_KEY;
use crate::staged::object_encoding::{DEPARTED_CLASS, DICTIONARY_STORAGE, departed_header};
use crate::value::Value;
use crate::value_vec::{CONSTANT_FORM, INLINE_PROPERTY_COUNT, ValueArena, ValueVec};

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

    /// The header word of `value`'s class cell when it is a live class cell (`live_class_cell`) of region 1: the class
    /// id, which an inline empty object claim takes as its header (`JsStepDispatch::empty_object_class`). That claim
    /// takes region 1 (`staged::bump::claim_class_cell`), so a class of another region is no answer.
    #[inline(always)]
    pub(crate) fn live_class_header(&self, value: Value) -> Option<u64> {
        let granule = self.live_class_cell(value)?;
        // The class alone: the cell's extras id (its header's high half) is its own, and a claim takes none.
        let class = header_class(self.class_space.header(granule));
        (layouts::class_region(class) == 1 && !layouts::class_is_dictionary(class)).then_some(u64::from(class))
    }

    /// The receiver key of a field entry for `value` when it is a live class cell outside dictionary
    /// storage and not under a constant mark: its class id OR'd with `CLASS_KEY`. A class id names the
    /// shape, the prototype link and the flags (`layouts::intern_class`), so the key fixes the cell's own
    /// slots, its holder and its flags; a flag change is a class change, so the key of the old class
    /// matches nothing. A frozen or non-extensible class keys: no write to it succeeds, so no store or add
    /// entry is ever filled from it (the fill is the edge). A constant-marked class has no key: a store
    /// through a key writes the field without `note_cell_write` and would leave the mark standing. `None`
    /// for any other value, a record, and a departed cell.
    #[inline(always)]
    pub(crate) fn live_class_key(&self, value: Value) -> Option<u32> {
        let granule = self.live_class_cell(value)?;
        let class = header_class(self.class_space.header(granule));
        (layouts::class_flags(class) & (DICTIONARY_STORAGE as u64 | CONSTANT_FORM as u64) == 0).then_some(class | CLASS_KEY)
    }

    /// The spill slot of live class `class`, in its region's cell (`layouts::spill_from`): `None` where its shape's
    /// fields fit the region's capacity. The shape length is the property table's, so the answer is per class,
    /// never a global constant.
    #[inline(always)]
    pub(crate) fn class_spill_from(&self, class: u32) -> Option<u32> {
        layouts::class_spill(class, self.properties.shape_length(layouts::class_shape(class)))
    }

    /// The field `slot` of the live class cell `value` names (`live_class_cell`), `None` otherwise.
    #[inline(always)]
    pub(crate) fn live_class_field(&self, value: Value, slot: usize) -> Option<Value> {
        let granule = self.live_class_cell(value)?;
        Some(self.class_slot_value(granule, slot))
    }

    /// The departure edge: the ordinary object `value` names becomes a record, if it is a live class
    /// cell. Returns the record id that names the object from now on; a record's or a departed
    /// cell's id is returned unchanged. `None` for a value that is no ordinary object.
    pub(crate) fn depart_class_object(&mut self, value: Value) -> Option<u32> {
        let granule = value.direct_object_index()?.granule();
        self.depart_class_cell(granule)
    }

    /// `depart_class_object` of the object whose identity is granule `granule`.
    pub(crate) fn depart_class_cell(&mut self, granule: u32) -> Option<u32> {
        let header = self.class_space.header(granule);
        if header_class(header) == DEPARTED_CLASS {
            return Some(header_record(header));
        }
        let class = header_class(header);
        let shape = layouts::class_shape(class);
        let length = self.properties.shape_length(shape);
        let spill = self.class_spill_from(class);
        let (fields, properties) = self.departure_storage(granule, shape, length, spill);
        let mut object = Object::with_property_storage(
            layouts::class_link(class).prototype(),
            properties,
            [Value::UNDEFINED; INLINE_PROPERTY_COUNT],
        );
        // The class's flags and prototype link; the storage form is the record's own.
        object.set_class(class);
        // The cell's extras move into the record with its object, which holds them from now on (`ExtrasSpace`).
        let extras_id = self.class_space.extras_id(granule);
        if extras_id != NO_EXTRAS {
            object.install_extras(self.extras.release(extras_id));
        }
        self.admit_object_contents(&object);
        let Some(record_id) = self.object_records.bump_id(&mut self.allocated) else {
            panic!("object records exhausted: the object space holds 2^30 records");
        };
        // SAFETY: the id is claimed uninitialized and written whole here, before any safepoint.
        unsafe { self.object_records.record_ptr(record_id).write(OrdinaryRecord { object, stub: granule }) };
        self.class_space.write_header(granule, departed_header(record_id));
        // The departed cell keeps its inline fields; its run is freed, and its slack is `Value::DELETED`.
        self.class_space.drop_run(granule, spill);
        self.class_space.lay_out(granule, None, &fields[..length.min(region_capacity(granule) as usize)]);
        self.properties.note_class_departure(shape);
        // A holder cell's departure ends its direct field reads (the holder-cell edge, R4-D1b(d)).
        // The edge raises the one service word (`request_collection`): the next service point (after
        // this op, `Vm::serviced`, or a poll) collects, and every collection drains the departure
        // first (`Vm::drop_departed_holder_entries`), so no read in execution tests it.
        if self.class_space.take_holder_cell(granule) {
            self.class_space.note_holder_departure();
            self.request_collection();
        }
        Some(record_id)
    }

    /// The fields of live cell `granule` (`length` of them, under `shape`) and the record storage that holds them, for a
    /// departure. A dictionary cell's table is the record's dictionary by the same id, so nothing is copied or moved;
    /// any other cell's words are the record's values.
    fn departure_storage(&mut self, granule: u32, shape: u32, length: usize, spill: Option<u32>) -> (Vec<Value>, ValueVec) {
        match self.dictionary_id_of_cell(granule) {
            Some(id) => {
                let table = self.properties.dictionary(id);
                let fields: Vec<Value> = (0..length).map(|slot| table.get(&(slot as u32)).copied().unwrap_or(Value::DELETED)).collect();
                (fields, ValueArena::with_dictionary(shape, id))
            }
            None => {
                let fields: Vec<Value> = (0..length).map(|slot| self.class_space.word(granule, spill, slot)).collect();
                let properties = self.properties.with_values(shape, &fields);
                (fields, properties)
            }
        }
    }

    /// Marks the live class cell `holder` as the holder of a field entry, so its departure is an edge
    /// the VM drains (`take_holder_departure`). A record holder needs no mark: its record stays put.
    pub(crate) fn note_holder_cell(&mut self, holder: Value) {
        if let Some(granule) = self.live_class_cell(holder) {
            self.class_space.mark_holder_cell(granule);
        }
    }

    /// Whether a holder cell departed since the last call; the caller clears the field caches.
    pub(crate) fn take_holder_departure(&mut self) -> bool {
        self.class_space.take_holder_departure()
    }

    /// The departure edge for a record-only view of `value` (`object_record_mut`, `object_and_arena_mut`,
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
    pub(super) fn note_cell_write(&mut self, granule: u32) {
        let class = header_class(self.class_space.header(granule));
        let mut object = Object::of_class(class);
        object.note_constant_write();
        if object.layout_id() != class {
            self.class_space.write_class(granule, object.layout_id());
        }
    }

    /// The notes of a write to slot `slot` of live cell `granule` (`Object::note_value_write`, for a cell): the
    /// constant mark clears (`note_cell_write`), a marked prototype's readers retire, and a global object's table
    /// notes the write. Only the cell's extras hold the last two, so a cell without extras has neither.
    pub(super) fn note_cell_value_write(&mut self, granule: u32, slot: usize) {
        self.note_cell_write(granule);
        let object = Object::of_class(header_class(self.class_space.header(granule)));
        let extras = self.class_space.extras_id(granule);
        if let Some(extras) = self.extras.get_mut(extras) {
            extras.note_value_write(&object, slot);
        }
    }

    /// A store or add of `value` at slot `slot` of live cell `granule`, whose spill slot is `from` before and `to`
    /// after (`ClassSpace::put_word`): the store gate's barrier for a heap reference, the constant mark's clear, then
    /// the word. A moved run (allocated or grown) fires the holder edge. A field holds the one Value word, which is
    /// already a double's raw bits under invariant I (c-design 4.1), so every field is stored and scanned alike.
    pub(super) fn class_cell_put(&mut self, granule: u32, from: Option<u32>, to: Option<u32>, slot: usize, value: Value) {
        if value.heap_index().is_some() {
            self.class_space.barrier(granule);
        }
        self.note_cell_value_write(granule, slot);
        if self.class_space.put_word(granule, from, to, slot, value) {
            self.holder_fields_moved(granule);
        }
    }

    /// A store of `value` to field `slot` of live cell `granule`, in place (`Heap::property_set`), through its run
    /// where the slot spills.
    pub(crate) fn class_cell_store(&mut self, granule: u32, slot: usize, value: Value) {
        if let Some(id) = self.dictionary_id_of_cell(granule) {
            return self.dictionary_cell_store(granule, id, slot, value);
        }
        let spill = self.live_spill_from(granule);
        self.class_cell_put(granule, spill, spill, slot, value);
    }

    /// The initialization of live cell `granule` with `values` under exact shape `shape`, in place: the fields take
    /// the stored words of `shape` (`ClassSpace::lay_out`, spilling past the region), and the cell takes the class of
    /// `shape` with its link and flags.
    pub(crate) fn class_cell_initialize(&mut self, granule: u32, shape: u32, values: &[Value]) {
        // The fields' words under `shape`: a Number-typed field holds its raw double (`stored_words`).
        let words = self.properties.stored_words(shape, values);
        if let Some(id) = self.dictionary_id_of_cell(granule) {
            return self.dictionary_cell_initialize(granule, id, shape, &words);
        }
        self.note_cell_layout_change(granule);
        if words.iter().any(|word| word.heap_index().is_some()) {
            self.class_space.barrier(granule);
        }
        self.note_cell_write(granule);
        let from = self.live_spill_from(granule);
        if self.class_space.lay_out(granule, from, &words) {
            self.holder_fields_moved(granule);
        }
        let class = header_class(self.class_space.header(granule));
        let next = layouts::intern_class(shape, layouts::class_link(class), layouts::class_flags(class), layouts::class_region(class));
        self.class_space.write_class(granule, next);
    }

    /// The add of `value` as field `slot` along the class edge from class `before`'s shape to shape `after`, in place
    /// for live cell `granule`: the cell keeps its link and flags, takes the after-shape's class, and the readers of
    /// the old shape retire (`Heap::set_class`). A field past the region spills into the cell's run (`class_cell_put`).
    pub(crate) fn class_cell_add(&mut self, granule: u32, before: u32, after: u32, slot: usize, value: Value) {
        if let Some(id) = self.dictionary_id_of_cell(granule) {
            return self.dictionary_cell_add(granule, id, after, slot, value);
        }
        // The add observes its value under the after-shape, the shape the add's slot belongs to, before the
        // field's word is taken: a widening rewrites the words already held, then this one is converted.
        self.note_cell_layout_change(granule);
        self.properties.note_shape_value(after, slot, value);
        self.rewrite_widened_numbers();
        let word = self.properties.stored_word(after, slot, value);
        let region = layouts::class_region(header_class(self.class_space.header(granule)));
        let from = self.shape_spill_from(region, before);
        let to = self.shape_spill_from(region, after);
        self.class_cell_put(granule, from, to, slot, word);
        let class = header_class(self.class_space.header(granule));
        debug_assert_eq!(layouts::class_shape(class), before, "an add's cell holds its transition's before-shape");
        // The cell never moves, so its region is the one the class it leaves names.
        let next = layouts::intern_class(after, layouts::class_link(class), layouts::class_flags(class), layouts::class_region(class));
        self.class_space.write_class(granule, next);
        self.set_class(before, after);
    }
}
