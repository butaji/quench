//! The dictionary space of the class space (departure-free S3): the property tables of class cells in dictionary
//! storage. A dictionary-mode cell's class carries `DICTIONARY_STORAGE`, and its slot 0 holds the dictionary word:
//! the id of its table here, as a `Value` whose bits are the id (`dictionary_word`). The table is the one the record
//! path keeps today (`ValueArena`'s dictionaries): slot index to stored word.
//!
//! The space is a slab of tables with a free list. A freed id is reused, and a freed table is empty. A cell names its
//! table by id, never by address, so the slab may grow and the heap stays plain data. The collector traces a live
//! cell's table through its dictionary word (`ClassSpace::dictionary_of`), and the sweep of a dead cell frees the id
//! (`dispose_dead_runs`).

use super::class_space::ClassSpace;
use super::layouts;
use crate::value::Value;
use rustc_hash::FxHashMap;

/// The table of a dictionary-mode cell: each slot index to its stored word (a deleted slot holds `Value::DELETED`).
pub(super) type DictionaryTable = FxHashMap<u32, Value>;

/// The tables of the dictionary-mode cells, by id.
#[derive(Default)]
pub(super) struct DictionarySpace {
    /// The table of each id. A freed id holds an empty table and is in `free`.
    tables: Vec<DictionaryTable>,
    /// The ids of freed tables, reused before the slab grows.
    free: Vec<u32>,
}

/// The word a dictionary-mode cell holds in slot 0 for table `id`.
pub(super) fn dictionary_word(id: u32) -> Value {
    Value::from_bits(u64::from(id))
}

/// The table id a dictionary word names.
#[inline(always)]
pub(super) fn dictionary_id_of(word: Value) -> u32 {
    word.to_bits() as u32
}

impl DictionarySpace {
    /// Stores `table` under a fresh id, reusing a freed one first, and returns the id.
    pub(super) fn alloc(&mut self, table: DictionaryTable) -> u32 {
        match self.free.pop() {
            Some(id) => {
                self.tables[id as usize] = table;
                id
            }
            None => {
                let id = u32::try_from(self.tables.len()).expect("dictionary tables exhausted");
                self.tables.push(table);
                id
            }
        }
    }

    /// The table of live id `id`.
    #[inline(always)]
    pub(super) fn get(&self, id: u32) -> &DictionaryTable {
        &self.tables[id as usize]
    }

    /// The table of live id `id`, exclusively.
    #[inline(always)]
    pub(super) fn get_mut(&mut self, id: u32) -> &mut DictionaryTable {
        &mut self.tables[id as usize]
    }

    /// Frees id `id`, returning its table so the caller may read the values it held.
    pub(super) fn take(&mut self, id: u32) -> DictionaryTable {
        self.free.push(id);
        std::mem::take(&mut self.tables[id as usize])
    }

    /// The tables and the free ids, as the heap snapshot keeps them (`heap::snapshot`).
    pub(super) fn snapshot_parts(&self) -> (&[DictionaryTable], &[u32]) {
        (&self.tables, &self.free)
    }

    /// The space of a snapshot's `snapshot_parts`.
    pub(super) fn from_snapshot(tables: Vec<DictionaryTable>, free: Vec<u32>) -> Self {
        Self { tables, free }
    }
}

impl ClassSpace {
    /// The id of the table dictionary cell `g` holds: its slot 0 word. The cell's class must be dictionary storage.
    #[inline(always)]
    pub(super) fn dictionary_id(&self, g: u32) -> u32 {
        dictionary_id_of(self.word(g, None, 0))
    }

    /// The table of live cell `g` of class `class`, when the class is dictionary storage: its word names it.
    /// `None` for an ordinary class cell. The space must be reserved.
    #[inline(always)]
    pub(super) fn dictionary_of(&self, g: u32, class: u32) -> Option<&DictionaryTable> {
        layouts::class_is_dictionary(class).then(|| self.dictionaries.get(self.dictionary_id(g)))
    }

    /// The cell words the collector traces for live cell `g` of class `class` under spill slot `spill`: the inline
    /// and run words, or none where the cell is a dictionary cell, whose edges are its table's (`dictionary_of`).
    pub(super) fn marking_parts(&self, g: u32, class: u32, spill: Option<u32>) -> (&[Value], &[Value], Option<&DictionaryTable>) {
        match self.dictionary_of(g, class) {
            Some(table) => (&[][..], &[][..], Some(table)),
            None => {
                let (fields, run) = self.cell_words(g, spill);
                (fields, run, None)
            }
        }
    }
}
