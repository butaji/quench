use super::*;

const STRING_ATOM_HASH_MULTIPLIER: usize = 0x9e37_79b1;

impl<H: Host> Vm<H> {
    /// Join two string values directly; primitive string identity is not observable.
    pub(super) fn intern_dynamic_concat(&mut self, left: Value, right: Value) -> Option<Value> {
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
        let value = self.heap.alloc_string(text);
        Some(value)
    }

    /// The atom of the string `key`, interned as `intern_js_atom` does. A string value names
    /// one immutable text until a collection, which empties the cache, so a key used again
    /// (a `for-in` key passed to `getOwnPropertyDescriptor`, `in` and `defineProperty`, as
    /// TypeScript's `__createBinding` does for each re-exported name) is not hashed and
    /// compared again. `None` when `key` is not a string.
    pub(super) fn string_key_atom(&mut self, key: Value) -> Option<Atom> {
        let slot = key.heap_index().map(|index| {
            (index as usize).wrapping_mul(STRING_ATOM_HASH_MULTIPLIER) & (STRING_ATOM_CACHE_SIZE - 1)
        });
        if let (Some(slot), Some(atoms)) = (slot, &self.string_atoms)
            && atoms[slot].0 == key
        {
            return Some(atoms[slot].1);
        }
        let name = self.heap.string(key)?.clone();
        let atom = self.intern_js_atom(&name);
        if let Some(slot) = slot {
            self.string_atoms
                .get_or_insert_with(|| Box::new([(Value::UNDEFINED, 0); STRING_ATOM_CACHE_SIZE]))[slot] =
                (key, atom);
        }
        Some(atom)
    }
}
