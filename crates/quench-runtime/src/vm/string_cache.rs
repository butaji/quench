use super::*;

/// The parts a `StringConcat` converts without a heap list: a template of up to four
/// substitutions and their text.
const STRING_CONCAT_INLINE_PARTS: usize = 9;

/// Entries of the direct-mapped `Vm::concat_memo`; a power of two.
pub(super) const CONCAT_MEMO_ENTRIES: usize = 64;
/// The most parts a memoized concatenation has.
const CONCAT_MEMO_PARTS: usize = 4;

/// One memoized `StringConcat`: its parts, by value identity, and the string they made.
/// Strings and Numbers are immutable, so the same parts make the same text, and a string's
/// identity is not observable: a concatenation that meets its parts again takes the earlier
/// result instead of converting, copying and allocating again. Cleared by every collection
/// (`Vm::collect_slow`), which may free a part's record and reuse its id.
#[derive(Clone, Copy)]
pub(super) struct ConcatMemoEntry {
    parts: [Value; CONCAT_MEMO_PARTS],
    count: usize,
    result: Value,
}

impl ConcatMemoEntry {
    pub(super) const EMPTY: Self = Self { parts: [Value::UNDEFINED; CONCAT_MEMO_PARTS], count: 0, result: Value::UNDEFINED };
}

impl<H: Host> Vm<H> {
    /// The memo entry the concatenation of `count` parts, each a Number or a string, maps to,
    /// with its parts; `None` for any other part or too many.
    #[inline(always)]
    fn concat_memo_key(&self, count: usize, part: &impl Fn(&Self, usize) -> Value) -> Option<(usize, [Value; CONCAT_MEMO_PARTS])> {
        if count == 0 || count > CONCAT_MEMO_PARTS {
            return None;
        }
        let mut parts = [Value::UNDEFINED; CONCAT_MEMO_PARTS];
        let mut hasher = rustc_hash::FxHasher::default();
        for (index, slot) in parts.iter_mut().enumerate().take(count) {
            let value = part(self, index);
            if value.as_number().is_none() && !self.heap.is_string(value) {
                return None;
            }
            *slot = value;
            std::hash::Hash::hash(&value, &mut hasher);
        }
        Some((((std::hash::Hasher::finish(&hasher) >> 40) as usize) & (CONCAT_MEMO_ENTRIES - 1), parts))
    }

    /// `string_concat`'s result for `parts` if it met them since the last collection.
    #[inline(always)]
    fn concat_memo_get(&self, index: usize, count: usize, parts: &[Value; CONCAT_MEMO_PARTS]) -> Option<Value> {
        let entry = &self.concat_memo[index];
        (entry.count == count && entry.parts == *parts).then_some(entry.result)
    }

    /// `ascii_string_concat` made into a string value, memoized (`ConcatMemoEntry`) where the
    /// parts allow: `None` where it has no result.
    #[inline(always)]
    pub(super) fn memoized_ascii_string_concat(&mut self, count: usize, part: impl Fn(&Self, usize) -> Value) -> Option<Value> {
        let Some((index, parts)) = self.concat_memo_key(count, &part) else {
            return self.ascii_string_concat(count, &part).map(|text| self.heap.alloc_string(text));
        };
        if let Some(result) = self.concat_memo_get(index, count, &parts) {
            return Some(result);
        }
        let result = self.heap.alloc_string(self.ascii_string_concat(count, &part)?);
        self.concat_memo[index] = ConcatMemoEntry { parts, count, result };
        Some(result)
    }

    /// `left + right` of strings and Numbers whose text is ASCII and short enough to sit
    /// inline (`SHORT_ASCII_CAPACITY`), such as a key built as `"d" + i`: each operand's
    /// bytes written once into the result, with no string made per operand or joined.
    pub(super) fn short_ascii_concat(&mut self, left: Value, right: Value) -> Option<Value> {
        let mut bytes = crate::number_to_string::Short::new();
        for operand in [left, right] {
            match self.heap.string(operand) {
                Some(text) => match text.existing_flat()?.view() {
                    super::wtf16::Units::Ascii(text) => bytes.try_extend(text)?,
                    super::wtf16::Units::Wide(_) => return None,
                },
                None if let Some(integer) = operand.as_int() => bytes.try_extend(itoa::Buffer::new().format(integer).as_bytes())?,
                None => {
                    let mut number = crate::number_to_string::Short::new();
                    crate::number_to_string::write(operand.as_number()?, &mut number);
                    bytes.try_extend(&number)?;
                }
            }
        }
        if bytes.len() > wtf16::SHORT_ASCII_CAPACITY {
            return None;
        }
        // SAFETY: every byte came from ASCII text or a Number's ASCII digits.
        let text = unsafe { std::str::from_utf8_unchecked(&bytes) };
        // Such a key usually spells an atom whose string lives: it is the result, made and
        // matched to its atom once rather than per concatenation and per keyed access.
        let atom = self.find_atom(Self::atom_hash_str(text), |atom| self.atom_units_equal_str(atom, text));
        if let Some(string) = atom.and_then(|atom| self.heap.atom_string(atom, false)) {
            return Some(string);
        }
        let string = self.heap.alloc_string(JsString::from_str(text));
        atom.inspect(|atom| self.heap.record_string_atom(string, *atom));
        Some(string)
    }

    /// Join two string values directly; primitive string identity is not observable.
    pub(super) fn intern_dynamic_concat(&mut self, left: Value, right: Value) -> Option<Value> {
        let (Some(left), Some(right)) = (self.heap.string(left), self.heap.string(right)) else {
            return None;
        };
        #[cfg(feature = "profile-aggregate")]
        let operand_shapes = (left.profile_shape(), right.profile_shape());
        let text = left.concat(right);
        #[cfg(feature = "profile-aggregate")]
        {
            self.profile.string_concat_shapes(operand_shapes.0, operand_shapes.1, text.profile_shape() != 2);
            self.profile.string_concat_size(text.len());
            self.profile.concat_cache(false);
        }
        Some(self.heap.alloc_string(text))
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

    /// `UNARY_TO_STRING_PRIMITIVE` (or, for a `+` chain's operand, `UNARY_ADD_OPERAND_STRING`:
    /// the same, the object's ToPrimitive taken with the default hint) of `value`: itself
    /// where ToString of it runs no guest code and raises nothing, else that ToString, made now.
    pub(super) fn string_primitive(&mut self, p: &ResidualProgram, operator: u32, value: Value) -> Result<Value, JsError> {
        if self.converts_to_string_purely(value) {
            return Ok(value);
        }
        let value = if operator == crate::bytecode::UNARY_ADD_OPERAND_STRING { self.to_primitive(p, value, "default")? } else { value };
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
        static BOUND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *BOUND.get_or_init(|| std::env::var_os("GCX_BOUND").is_some()) && count >= 3 && self.heap.is_string(part(self, 0)) {
            return Ok(part(self, 0));
        }
        if let Some(value) = self.memoized_ascii_string_concat(count, &part) {
            return Ok(value);
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
