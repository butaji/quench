//! Reading the arena without changing it: heap references, duplicates, and the census.
use super::*;

impl ValueArena {
    /// The heap references among a vector's values: the edges a collector traces. Numbers,
    /// other immediates and deleted slots are not references. An empty vector without a range,
    /// as every array without named properties has, is answered inline: a mark visit makes
    /// this call per array (V8-v7 Splay: 2.75M calls a run, ~31 Ir each).
    #[inline(always)]
    pub(crate) fn append_heap_references(&self, vector: ValueVec, output: &mut Vec<Value>) {
        if vector.holds_no_values() {
            return;
        }
        self.append_held_heap_references(vector, output);
    }

    #[inline(never)]
    pub(super) fn append_held_heap_references(&self, vector: ValueVec, output: &mut Vec<Value>) {
        let len = self.len(vector);
        if vector.is_dictionary() {
            let values = &self.dictionaries[&vector.dictionary_id()];
            output.extend(
                (0..len)
                    .filter_map(|slot| values.get(&(slot as u32)).copied())
                    .filter(|value| value.is_heap()),
            );
            return;
        }
        if !self.has_compact_range(vector) {
            return;
        }
        if vector.is_narrow() {
            // SAFETY: each index is one of the live vector's values.
            output.extend((0..len).map(|index| unsafe { self.narrow_get(vector, index) }).filter(|value| value.is_heap()));
            return;
        }
        output.extend(
            self.values[vector.start()..vector.start() + len]
                .iter()
                .copied()
                .filter(|value| value.is_heap()),
        );
    }

    /// An independent copy of `vector`'s storage: the same shape, integrity and values in a
    /// range or dictionary of its own, so neither owner's release, growth or writes reach the
    /// other. Inline storage lives in its owner's header and an empty vector has no range, so
    /// both are returned as they are.
    pub(crate) fn duplicate(&mut self, vector: ValueVec) -> ValueVec {
        let mut copy = vector;
        if vector.has_inline_property_storage() {
            return copy;
        }
        if vector.is_dictionary() {
            let slots = self.dictionaries[&vector.dictionary_id()].clone();
            copy.use_dictionary(self.insert_dictionary(slots));
            return copy;
        }
        if vector.is_narrow() {
            // The copy's owner may hold extras or be an array, which take wide runs only.
            let values = self.narrow_values(vector);
            copy.auxiliary &= !NARROW_FORM;
            copy.start = copy.start & !START_MASK | EMPTY_START;
            if !values.is_empty() {
                match self.run_of(&values) {
                    Some(start) => copy.start = start | (copy.start & !START_MASK),
                    None => copy.use_dictionary(self.allocate_dictionary(values)),
                }
            }
            return copy;
        }
        let capacity = self.capacity(vector);
        if capacity == 0 {
            return copy;
        }
        let len = self.len(vector);
        match self.allocate(capacity, MAX_ARENA_START) {
            Some(start) => {
                self.values.copy_within(vector.start()..vector.start() + len, start);
                copy.relocate(start);
            }
            None => {
                let values = self.decoded_run(vector.start(), len);
                copy.use_dictionary(self.allocate_dictionary(values));
            }
        }
        copy
    }

    /// Frees `vector`'s storage. A young run (`Nursery`) returns with the rest of the young
    /// range when its collection ends, so its release is the range test alone.
    /// The arena's words in use, released ranges included (`QUENCH_TRACE=storage-census`).
    pub(crate) fn resident_words(&self) -> usize {
        self.values.len()
    }

    /// Its length, capacity, released values, held values against the last full collection's
    /// (`GenerationalPacing::arena_growth_reached`) and nursery range, for `QUENCH_TRACE=gc`.
    pub(crate) fn trace_summary(&self) -> String {
        let n = self.nursery;
        let free_classes = self.free.iter().enumerate().filter(|(_, list)| !list.is_empty())
            .map(|(class, list)| format!("{class}:{}", list.len())).collect::<Vec<_>>().join(",");
        format!("arena_len={} arena_cap={} released={} arena_held={} arena_settled={} nursery={}..{}..{} free=[{free_classes}]",
            self.values.len(), self.values.capacity(), self.released_values, self.occupied_values(), self.settled_values,
            n.floor, n.top, n.end)
    }
}
