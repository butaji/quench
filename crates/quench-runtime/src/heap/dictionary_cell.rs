//! Dictionary storage of class cells (departure-free S3). An ordinary object that goes to dictionary mode while it is a
//! live class cell stays one: its class takes `DICTIONARY_STORAGE` and the dictionary shape, and its slot 0 holds the
//! dictionary word, the id of its table in the value arena's dictionary store (`ValueArena::insert_dictionary`), the
//! one store records keep theirs in. Every write and add to such a cell goes to that table, and no departure is made
//! for it. Only `delete` still departs one (`Heap::depart_class_cell`), and the departure hands the same id to the
//! record, so the table is never copied or moved.

use super::Heap;
use super::class_space::{header_class, live_header};
use super::layouts;
use crate::value::Value;
use crate::value_vec::DictionaryTable;

/// The word a dictionary-mode cell holds in slot 0 for dictionary `id`.
pub(super) fn dictionary_word(id: u32) -> Value {
    Value::from_bits(u64::from(id))
}

/// The dictionary id a dictionary word names.
#[inline(always)]
pub(super) fn dictionary_id_of(word: Value) -> u32 {
    word.to_bits() as u32
}

impl Heap {
    /// The dictionary id of live cell `granule` when its class is dictionary storage: its slot 0 word.
    #[inline(always)]
    pub(crate) fn dictionary_id_of_cell(&self, granule: u32) -> Option<u32> {
        let class = header_class(self.class_space.header(granule));
        layouts::class_is_dictionary(class).then(|| dictionary_id_of(self.class_space.word(granule, None, 0)))
    }

    /// The table of live cell `granule` when its class is dictionary storage; `None` for an ordinary class cell.
    #[inline(always)]
    pub(super) fn cell_dictionary(&self, granule: u32) -> Option<&DictionaryTable> {
        let id = self.dictionary_id_of_cell(granule)?;
        Some(self.properties.dictionary(id))
    }

    /// The edges the collector traces for live cell `granule` of class `class`: its inline and run words, or none for a
    /// dictionary cell, whose edges are its table's.
    pub(super) fn cell_marking_parts(&self, granule: u32, class: u32) -> (&[Value], &[Value], Option<&DictionaryTable>) {
        if layouts::class_is_dictionary(class) {
            return (&[][..], &[][..], self.cell_dictionary(granule));
        }
        let (fields, run) = self.class_space.cell_words(granule, self.class_spill_from(class));
        (fields, run, None)
    }

    /// The live class cell `value` names when its class is dictionary storage, with its table's id.
    #[inline(always)]
    pub(crate) fn live_dictionary_cell(&self, value: Value) -> Option<(u32, u32)> {
        let granule = self.live_class_cell(value)?;
        self.dictionary_id_of_cell(granule).map(|id| (granule, id))
    }

    /// Rewrites the class word of live cell `granule` to the dictionary shape `shape`, keeping its link, region and flags,
    /// and retires the readers of the shape it leaves (`set_class`).
    fn cell_takes_shape(&mut self, granule: u32, shape: u32) {
        let class = header_class(self.class_space.header(granule));
        let before = layouts::class_shape(class);
        let next = layouts::intern_class(shape, layouts::class_link(class), layouts::class_flags(class), layouts::class_region(class));
        self.class_space.write_header(granule, live_header(next));
        self.set_class(before, shape);
    }

    /// The dictionary transition of live cell `granule` to dictionary shape `shape` (the shape `mark_object_dictionary`
    /// appended, over the cell's shape). The cell's fields move into a new table in the arena, its spill run is freed,
    /// and its class takes `DICTIONARY_STORAGE`. A marked holder's fields moved, so the holder edge fires (`holder_fields_moved`).
    pub(crate) fn cell_to_dictionary(&mut self, granule: u32, shape: u32) {
        let class = header_class(self.class_space.header(granule));
        let length = self.properties.shape_length(layouts::class_shape(class));
        let spill = self.class_spill_from(class);
        let table: DictionaryTable = (0..length).map(|slot| (slot as u32, self.class_space.word(granule, spill, slot))).collect();
        let id = self.properties.insert_dictionary(table);
        self.class_space.drop_run(granule, spill);
        self.class_space.lay_out(granule, None, &[dictionary_word(id)]);
        let flags = layouts::class_flags(class) | u64::from(crate::staged::object_encoding::DICTIONARY_STORAGE);
        let next = layouts::intern_class(shape, layouts::class_link(class), flags, layouts::class_region(class));
        self.class_space.write_header(granule, live_header(next));
        self.set_class(layouts::class_shape(class), shape);
        self.holder_fields_moved(granule);
    }

    /// Sets the shape of dictionary cell `granule`'s table-backed object to `shape`, when `value` names one: the table
    /// keeps its values, and only the class's shape changes. Returns whether it did.
    pub(crate) fn dictionary_set_shape(&mut self, value: Value, shape: u32) -> bool {
        let Some((granule, _)) = self.live_dictionary_cell(value) else {
            return false;
        };
        self.cell_takes_shape(granule, shape);
        true
    }

    /// A store of `value` (an already stored word) to slot `slot` of dictionary cell `granule` with table `id`. The
    /// store barriers the cell as an ordinary store does, then the table holds the word.
    pub(super) fn dictionary_cell_store(&mut self, granule: u32, id: u32, slot: usize, value: Value) {
        if value.heap_index().is_some() {
            self.class_space.barrier(granule);
        }
        self.note_cell_write(granule);
        self.properties.dictionary_mut(id).insert(slot as u32, value);
    }

    /// The add of `value` as slot `slot` of dictionary cell `granule` (table `id`) to shape `after`: the value is observed
    /// under `after`, the table takes its stored word, and the class takes the after-shape.
    pub(crate) fn dictionary_cell_add(&mut self, granule: u32, id: u32, after: u32, slot: usize, value: Value) {
        self.properties.note_shape_value(after, slot, value);
        self.rewrite_widened_numbers();
        let word = self.properties.stored_word(after, slot, value);
        if word.heap_index().is_some() {
            self.class_space.barrier(granule);
        }
        self.note_cell_write(granule);
        self.properties.dictionary_mut(id).insert(slot as u32, word);
        self.cell_takes_shape(granule, after);
    }

    /// The initialization of dictionary cell `granule` (table `id`) with the stored `words` of shape `shape`: the table
    /// is replaced by the words, and the class takes `shape`. The caller passed the words through `stored_words`.
    pub(crate) fn dictionary_cell_initialize(&mut self, granule: u32, id: u32, shape: u32, words: &[Value]) {
        if words.iter().any(|word| word.heap_index().is_some()) {
            self.class_space.barrier(granule);
        }
        self.note_cell_write(granule);
        let table = self.properties.dictionary_mut(id);
        table.clear();
        table.extend(words.iter().enumerate().map(|(slot, &word)| (slot as u32, word)));
        self.cell_takes_shape(granule, shape);
    }
}
