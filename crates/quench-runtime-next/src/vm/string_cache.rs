use super::*;

const STRING_CONCAT_HASH_MULTIPLIER: usize = 0x9e37_79b1;

impl<H: Host> Vm<H> {
    pub(super) fn intern_dynamic_concat(&mut self, left: Value, right: Value) -> Option<Value> {
        let left_index = left.heap_index()? as usize;
        let right_index = right.heap_index()? as usize;
        let cache_index = (left_index.wrapping_mul(STRING_CONCAT_HASH_MULTIPLIER) ^ right_index)
            & (STRING_CONCAT_CACHE_SIZE - 1);
        if let Some(entry) = self
            .string_concats
            .as_ref()
            .map(|concats| concats[cache_index])
            && entry.left == left
            && entry.right == right
            && matches!(self.heap.get(entry.result), Some(Cell::String(_)))
        {
            #[cfg(feature = "profile-aggregate")]
            self.profile.concat_cache(true);
            return Some(entry.result);
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
        let value = self.intern_dynamic_value(text);
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
