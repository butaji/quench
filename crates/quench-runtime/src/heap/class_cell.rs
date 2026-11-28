//! The class cell of an ordinary object, for its whole life: the reads, stores, adds and class edits that keep it
//! one (departure-free S7a). An ordinary object is born a cell (`Heap::insert_object_as_cell`) and stays one: its class
//! is its shape, prototype link and flags; a field past the region's capacity spills (`heap::spill`); a dictionary-mode
//! object's class carries its table (`heap::dictionary_cell`); a delete is a class transition (`heap::class_delete`).
//! No edit moves an object to a record, so a cell granule is always live.
//!
//! A dictionary-mode object takes its dictionary class from the VM (`Vm::mark_object_dictionary`), because a
//! dictionary shape lives in the VM's shape table.

use super::cell::Object;
use super::class_space::{header_class, is_cell_granule};
use super::{Heap, layouts};
use crate::staged::field_cache_layout::CLASS_KEY;
use crate::staged::object_encoding::DICTIONARY_STORAGE;
use crate::value::Value;

impl Heap {
    /// The granule of `value` when it names a class cell: one in a cell region. Every cell granule is live, since
    /// no cell ever stops being one. `None` for a record, a stub and any non-object.
    #[inline(always)]
    pub(crate) fn live_class_cell(&self, value: Value) -> Option<u32> {
        let granule = value.direct_object_index()?.granule();
        is_cell_granule(granule).then_some(granule)
    }

    /// The header word of `value`'s class cell when it is a class cell (`live_class_cell`) of region 1: the class
    /// id, which an inline empty object claim takes as its header (`JsStepDispatch::empty_object_class`). That claim
    /// takes region 1 (`staged::bump::claim_class_cell`), so a class of another region is no answer.
    #[inline(always)]
    pub(crate) fn live_class_header(&self, value: Value) -> Option<u64> {
        let granule = self.live_class_cell(value)?;
        // The class alone: the cell's extras id (its header's high half) is its own, and a claim takes none.
        let class = header_class(self.class_space.header(granule));
        (layouts::class_region(class) == 1 && !layouts::class_is_dictionary(class)).then_some(u64::from(class))
    }

    /// The receiver key of a field entry for `value` when it is a class cell outside dictionary
    /// storage: its class id OR'd with `CLASS_KEY`. A class id names the
    /// shape, the prototype link and the flags (`layouts::intern_class`), so the key fixes the cell's own
    /// slots, its holder and its flags; a flag change is a class change, so the key of the old class
    /// matches nothing. A frozen or non-extensible class keys: no write to it succeeds, so no store or add
    /// entry is ever filled from it (the fill is the edge). `None` for any other value, and for a record.
    #[inline(always)]
    pub(crate) fn live_class_key(&self, value: Value) -> Option<u32> {
        let granule = self.live_class_cell(value)?;
        let class = header_class(self.class_space.header(granule));
        (layouts::class_flags(class) & (DICTIONARY_STORAGE as u64) == 0).then_some(class | CLASS_KEY)
    }

    /// The spill slot of live class `class`, in its region's cell (`layouts::spill_from`): `None` where its shape's
    /// fields fit the region's capacity. The shape length is the property table's, so the answer is per class,
    /// never a global constant.
    #[inline(always)]
    pub(crate) fn class_spill_from(&self, class: u32) -> Option<u32> {
        layouts::class_spill(class, self.properties.shape_length(layouts::class_shape(class)))
    }

    /// The field `slot` of the class cell `value` names (`live_class_cell`), `None` otherwise.
    #[inline(always)]
    pub(crate) fn live_class_field(&self, value: Value, slot: usize) -> Option<Value> {
        let granule = self.live_class_cell(value)?;
        Some(self.class_slot_value(granule, slot))
    }

    /// Marks the class cell `holder` as the holder of a field entry, so a move of its fields is an edge the VM drains
    /// (`take_holder_moves`). A record holder needs no mark: its fields never move.
    pub(crate) fn note_holder_cell(&mut self, holder: Value) {
        if let Some(granule) = self.live_class_cell(holder) {
            self.class_space.mark_holder_cell(granule);
        }
    }

    /// Whether a marked holder's fields moved since the last call; the caller clears the field caches.
    pub(crate) fn take_holder_moves(&mut self) -> bool {
        self.class_space.take_holder_moves()
    }

    /// The append of `value` to class cell `granule` (not dictionary storage), which moves it to `shape`: the new slot
    /// is the one after its shape's last, and the cell takes `shape`'s class in place (`class_cell_add`).
    pub(crate) fn class_cell_append(&mut self, granule: u32, value: Value, shape: u32) {
        let before = layouts::class_shape(header_class(self.class_space.header(granule)));
        let slot = self.properties.shape_length(before);
        self.class_cell_add(granule, before, shape, slot, value);
    }

    /// The notes of a write to slot `slot` of class cell `granule` (`Object::note_value_write`, for a cell): a
    /// prototype's readers retire, and a global object's table notes the write. Only the cell's extras hold the
    /// last two, so a cell without extras has neither.
    pub(super) fn note_cell_value_write(&mut self, granule: u32, slot: usize) {
        let object = Object::of_class(header_class(self.class_space.header(granule)));
        let extras = self.class_space.extras_id(granule);
        if let Some(extras) = self.extras.get_mut(extras) {
            extras.note_value_write(&object, slot);
        }
    }

    /// A store or add of `value` at slot `slot` of class cell `granule`, whose spill slot is `from` before and `to`
    /// after (`ClassSpace::put_word`): the store gate's barrier for a heap reference, the value write's notes, then
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

    /// A store of `value` to field `slot` of class cell `granule`, in place (`Heap::property_set`), through its run
    /// where the slot spills.
    pub(crate) fn class_cell_store(&mut self, granule: u32, slot: usize, value: Value) {
        if let Some(id) = self.dictionary_id_of_cell(granule) {
            return self.dictionary_cell_store(granule, id, slot, value);
        }
        let spill = self.live_spill_from(granule);
        self.class_cell_put(granule, spill, spill, slot, value);
    }

    /// The initialization of class cell `granule` with `values` under exact shape `shape`, in place: the fields take
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
        let from = self.live_spill_from(granule);
        if self.class_space.lay_out(granule, from, &words) {
            self.holder_fields_moved(granule);
        }
        let class = header_class(self.class_space.header(granule));
        let next = layouts::intern_class(shape, layouts::class_link(class), layouts::class_flags(class), layouts::class_region(class));
        self.class_space.write_class(granule, next);
    }

    /// The add of `value` as field `slot` along the class edge from class `before`'s shape to shape `after`, in place
    /// for class cell `granule`: the cell keeps its link and flags, takes the after-shape's class, and the readers of
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
