use super::*;

const STRING_CONCAT_HASH_MULTIPLIER: usize = 0x9e37_79b1;

impl<H: Host> Vm<H> {
    /// `left + right` of strings and Numbers whose text is ASCII and short enough to sit
    /// inline (`SHORT_ASCII_CAPACITY`), such as a key built as `"d" + i`: each operand's
    /// bytes written once into the result, with no string made per operand or joined.
    pub(super) fn short_ascii_concat(&mut self, left: Value, right: Value) -> Option<Value> {
        let mut bytes = crate::number_to_string::Short::new();
        for operand in [left, right] {
            match self.heap.string(operand) {
                Some(text) => match text.existing_flat()?.view() {
                    super::wtf16::Units::Ascii(text) if text.len() <= wtf16::SHORT_ASCII_CAPACITY => {
                        bytes.extend_from_slice(text);
                    }
                    _ => return None,
                },
                None => crate::number_to_string::write(operand.as_number()?, &mut bytes),
            }
        }
        if bytes.len() > wtf16::SHORT_ASCII_CAPACITY {
            return None;
        }
        // SAFETY: every byte came from ASCII text or a Number's ASCII digits.
        let text = unsafe { std::str::from_utf8_unchecked(&bytes) };
        Some(self.heap.alloc_string(JsString::from_str(text)))
    }

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
            let text = left.concat(right);
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

    /// The atom of the string `key`, interned as `intern_js_atom` does, found once per string
    /// record and then read from it (`Heap::string_atom`). `None` when `key` is not a string.
    pub(super) fn string_key_atom(&mut self, key: Value) -> Option<Atom> {
        if let Some(atom) = self.heap.string_atom(key) {
            return Some(atom);
        }
        let name = self.heap.string(key)?.clone();
        let atom = self.intern_js_atom(&name);
        self.heap.record_string_atom(key, atom);
        Some(atom)
    }

    /// ToString of `value` as a string value: a string is itself, read and copied no further.
    pub(super) fn string_of_value(&mut self, p: &ResidualProgram, value: Value) -> Result<Value, JsError> {
        if self.heap.is_string(value) {
            return Ok(value);
        }
        let text = self.coerce_js_string(p, value)?;
        Ok(self.heap.alloc_string(text))
    }
}
