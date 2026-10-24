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
            if let (Some(left), Some(right), Some(result_text)) = (
                self.heap.string(left),
                self.heap.string(right),
                self.heap.string(result),
            ) {
                self.profile.string_concat_shapes(
                    left.profile_shape(),
                    right.profile_shape(),
                    result_text.profile_shape() != 2,
                );
            }
            #[cfg(feature = "profile-aggregate")]
            self.profile.concat_cache(true);
            return Some(result);
        }
        let text = {
            let (Some(left), Some(right)) = (self.heap.string(left), self.heap.string(right))
            else {
                return None;
            };
            #[cfg(feature = "profile-aggregate")]
            let operand_shapes = (left.profile_shape(), right.profile_shape());
            let mut text = left.clone();
            text.push_js_string(right);
            #[cfg(feature = "profile-aggregate")]
            self.profile.string_concat_shapes(
                operand_shapes.0,
                operand_shapes.1,
                text.profile_shape() != 2,
            );
            #[cfg(feature = "profile-aggregate")]
            self.profile.string_concat_size(text.len());
            text
        };
        #[cfg(feature = "profile-aggregate")]
        self.profile.concat_cache(false);
        // The operand-pair cache above already avoids repeating this concat.
        // Interning the result would hash every UTF-16 unit, which turns
        // repeated `out += chunk` into quadratic work for a growing string.
        // Primitive string identity is not observable, so store this rope
        // directly in the heap.
        let value = self.heap.alloc_string(text);
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

    /// The atom of the string `key`, interned as `intern_js_atom` does. A string value names
    /// one immutable text until a collection, which empties the cache, so a key used again
    /// (a `for-in` key passed to `getOwnPropertyDescriptor`, `in` and `defineProperty`, as
    /// TypeScript's `__createBinding` does for each re-exported name) is not hashed and
    /// compared again. `None` when `key` is not a string.
    pub(super) fn string_key_atom(&mut self, key: Value) -> Option<Atom> {
        let slot = string_atom_slot(key);
        if let (Some(slot), Some(atoms)) = (slot, &self.string_atoms)
            && atoms[slot].0 == key
        {
            return Some(atoms[slot].1);
        }
        let name = self.heap.string(key)?.clone();
        let atom = self.intern_js_atom(&name);
        self.note_string_atom(key, atom);
        Some(atom)
    }

    /// Records that the string `key` spells `atom`, as `string_key_atom` would find. A key
    /// made from its atom (`atom_string`: a `for-in` or `Object.keys` key) is then not hashed
    /// on its first use as a key either.
    pub(super) fn note_string_atom(&mut self, key: Value, atom: Atom) {
        if let Some(slot) = string_atom_slot(key) {
            self.string_atoms
                .get_or_insert_with(|| Box::new([(Value::UNDEFINED, 0); STRING_ATOM_CACHE_SIZE]))[slot] =
                (key, atom);
        }
    }
}

/// The `string_atoms` slot of the heap string `key`.
fn string_atom_slot(key: Value) -> Option<usize> {
    key.heap_index()
        .map(|index| (index as usize).wrapping_mul(STRING_CONCAT_HASH_MULTIPLIER) & (STRING_ATOM_CACHE_SIZE - 1))
}
