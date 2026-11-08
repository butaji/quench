use super::*;

const STRING_CONCAT_HASH_MULTIPLIER: usize = 0x9e37_79b1;
/// The parts a `StringConcat` converts without a heap list: a template of up to four
/// substitutions and their text.
const STRING_CONCAT_INLINE_PARTS: usize = 9;

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

    /// Whether ToString of `value` runs no guest code and raises nothing: a string, number,
    /// boolean, null or undefined.
    #[inline(always)]
    pub(super) fn converts_to_string_purely(&self, value: Value) -> bool {
        let immediate = value.as_bool().is_some() || value.is_null() || value.is_undefined();
        immediate || value.as_number().is_some() || self.heap.is_string(value)
    }

    /// `UNARY_TO_STRING_PRIMITIVE` of `value`: itself where ToString of it runs no guest code
    /// and raises nothing, else that ToString, made now.
    pub(super) fn string_primitive(&mut self, p: &ResidualProgram, value: Value) -> Result<Value, JsError> {
        if self.converts_to_string_purely(value) {
            return Ok(value);
        }
        let text = self.coerce_js_string(p, value)?;
        Ok(self.heap.alloc_string(text))
    }

    /// `StringConcat` of `count` parts that `part` reads by position, each read when it is
    /// converted: ToString of each, in order, as one string.
    #[inline(always)]
    pub(super) fn string_concat(
        &mut self,
        p: &ResidualProgram,
        count: usize,
        part: impl Fn(&Self, usize) -> Value,
    ) -> Result<Value, JsError> {
        if let Some(text) = self.ascii_string_concat(count, &part) {
            return Ok(self.heap.alloc_string(text));
        }
        self.converted_string_concat(p, count, part)
    }

    /// `StringConcat` of the register `window` of frame `f`.
    pub(super) fn frame_string_concat(&mut self, p: &ResidualProgram, f: usize, window: crate::bytecode::RegisterWindow) -> Result<Value, JsError> {
        let start = usize::from(window.base);
        self.string_concat(p, usize::from(window.count), |vm, part| vm.frames[f].registers[start + part])
    }

    /// `string_concat` of parts that are numbers and flat ASCII strings, short together: each
    /// number written on the stack, then every part copied once into the result's storage.
    /// `None` for any other part, which `converted_string_concat` converts.
    #[inline(always)]
    pub(super) fn ascii_string_concat(&self, count: usize, part: impl Fn(&Self, usize) -> Value) -> Option<JsString> {
        enum Piece<'text> {
            Text(&'text [u8]),
            Number(crate::number_to_string::Short),
        }
        let mut pieces = smallvec::SmallVec::<[Piece<'_>; STRING_CONCAT_INLINE_PARTS]>::new();
        let mut length = 0;
        for index in 0..count {
            let value = part(self, index);
            let piece = match value.as_number() {
                Some(number) => {
                    let mut text = crate::number_to_string::Short::new();
                    crate::number_to_string::write(number, &mut text);
                    Piece::Number(text)
                }
                None => match self.heap.string(value)?.existing_flat()?.view() {
                    super::wtf16::Units::Ascii(bytes) => Piece::Text(bytes),
                    super::wtf16::Units::Wide(_) => return None,
                },
            };
            length += match &piece {
                Piece::Text(bytes) => bytes.len(),
                Piece::Number(text) => text.len(),
            };
            pieces.push(piece);
        }
        JsString::copies_whole(length).then(|| {
            JsString::from_ascii_parts(length, pieces.iter().map(|piece| match piece {
                Piece::Text(bytes) => *bytes,
                Piece::Number(text) => text.as_slice(),
            }))
        })
    }

    /// `string_concat` of any parts: each converted by ToString in order, then joined.
    pub(super) fn converted_string_concat(
        &mut self,
        p: &ResidualProgram,
        count: usize,
        part: impl Fn(&Self, usize) -> Value,
    ) -> Result<Value, JsError> {
        let mut texts = smallvec::SmallVec::<[JsString; STRING_CONCAT_INLINE_PARTS]>::new();
        for index in 0..count {
            let value = part(self, index);
            texts.push(self.coerce_js_string(p, value)?);
        }
        let text = self.string_build_result(p, JsString::concat_all(&texts))?;
        Ok(self.heap.alloc_string(text))
    }
}
