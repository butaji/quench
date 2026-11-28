//! Dictionary storage: the tables a vector in dictionary mode names by id.
use super::*;

impl ValueArena {
    #[cold]
    #[inline(never)]
    pub(super) fn dictionary_get(&self, vector: ValueVec, index: usize) -> Value {
        self.dictionaries[&vector.dictionary_id()][&(index as u32)]
    }

    #[cold]
    #[inline(never)]
    pub(super) fn dictionary_set(&mut self, vector: ValueVec, index: usize, value: Value) {
        self.dictionaries
            .get_mut(&vector.dictionary_id())
            .expect("object dictionary exists")
            .insert(index as u32, value);
    }

    /// The storage of an object in dictionary mode whose table is dictionary `id` (`insert_dictionary`). A dictionary-mode
    /// class cell names the same table by its id, so a departure hands that id to its record (`heap::departure`).
    pub(crate) fn with_dictionary(shape: u32, id: u32) -> ValueVec {
        let mut vector = ValueVec { start: EMPTY_START, auxiliary: shape };
        vector.use_dictionary(id);
        vector
    }

    pub(super) fn allocate_dictionary(&mut self, values: Vec<Value>) -> u32 {
        self.insert_dictionary(
            values
                .into_iter()
                .enumerate()
                .map(|(slot, value)| (slot as u32, value))
                .collect(),
        )
    }

    /// Stores `slots` (slot index to stored word) as a new dictionary table, reusing a released id first, and returns
    /// its id. The arena is the one dictionary store: a record's dictionary and a dictionary-mode class cell's name
    /// their tables here.
    pub(crate) fn insert_dictionary(&mut self, slots: DictionaryTable) -> u32 {
        let id = self.free_dictionaries.pop().unwrap_or_else(|| {
            let id = self.next_dictionary;
            self.next_dictionary = id
                .checked_add(1)
                .expect("property dictionary table exhausted");
            assert!(id <= START_MASK, "property dictionary table exhausted");
            id
        });
        self.dictionaries.insert(id, slots);
        id
    }

    /// The table of dictionary `id`, which a record or a dictionary-mode class cell names.
    #[inline(always)]
    pub(crate) fn dictionary(&self, id: u32) -> &DictionaryTable {
        self.dictionaries.get(&id).expect("object dictionary exists")
    }

    /// The table of dictionary `id`, exclusively.
    #[inline(always)]
    pub(crate) fn dictionary_mut(&mut self, id: u32) -> &mut DictionaryTable {
        self.dictionaries.get_mut(&id).expect("object dictionary exists")
    }

    /// Frees dictionary `id` with its owner: a record's release, or the sweep of a dead dictionary-mode class cell.
    pub(crate) fn release_dictionary(&mut self, id: u32) {
        self.dictionaries.remove(&id);
        self.free_dictionaries.push(id);
    }
}
