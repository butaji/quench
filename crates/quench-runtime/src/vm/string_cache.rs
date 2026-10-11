use super::*;

const STRING_CONCAT_HASH_MULTIPLIER: usize = 0x9e37_79b1;

impl<H: Host> Vm<H> {
    /// The cache slot for an operand pair.
    #[inline(always)]
    fn concat_cache_index(left: Value, right: Value) -> Option<usize> {
        let left_index = left.heap_index()? as usize;
        let right_index = right.heap_index()? as usize;
        Some(
            (left_index.wrapping_mul(STRING_CONCAT_HASH_MULTIPLIER) ^ right_index)
                & (STRING_CONCAT_CACHE_SIZE - 1),
        )
    }

    /// A cached `left + right` of two strings. Entries are made only for string operands and
    /// collection empties the cache, so between collections a matching pair names the same
    /// immutable string cells and the stored result is their live concatenation.
    #[inline(always)]
    pub(super) fn cached_concat(&self, left: Value, right: Value) -> Option<Value> {
        let entry = self.string_concats.as_ref()?[Self::concat_cache_index(left, right)?];
        (entry.left == left && entry.right == right).then_some(entry.result)
    }

    pub(super) fn intern_dynamic_concat(&mut self, left: Value, right: Value) -> Option<Value> {
        let cache_index = Self::concat_cache_index(left, right)?;
        if let Some(result) = self.cached_concat(left, right) {
            #[cfg(feature = "profile-aggregate")]
            self.profile.concat_cache(true);
            return Some(result);
        }
        let text = {
            let (Some(Cell::String(left)), Some(Cell::String(right))) =
                (self.heap.get(left), self.heap.get(right))
            else {
                return None;
            };
            let mut text = left.clone();
            text.push_js_string(right);
            #[cfg(feature = "profile-aggregate")]
            self.profile.string_concat_size(text.units().len());
            text
        };
        #[cfg(feature = "profile-aggregate")]
        self.profile.concat_cache(false);
        // Concatenation results are fresh strings; string equality compares units, and the
        // operand-pair cache above already reuses repeated concatenations.
        let value = self.heap.alloc(Cell::String(text));
        let cache = self.string_concats.get_or_insert_with(|| {
            vec![EMPTY_STRING_CONCAT_CACHE; STRING_CONCAT_CACHE_SIZE].into_boxed_slice()
        });
        cache[cache_index] = StringConcatCache {
            left,
            right,
            result: value,
        };
        Some(value)
    }
}
