use super::*;

/// The parts a `StringConcat` converts without a heap list: a template of up to four
/// substitutions and their text.
const STRING_CONCAT_INLINE_PARTS: usize = 9;
/// The most parts `string_of_parts` reads before it knows the window is too long for it.
/// The most text a string of parts may hold in its known parts: past it the copy a flat
/// concatenation makes costs more than its record saves, and a string read soon is copied twice.
const PARTS_KNOWN_LIMIT: usize = 256;
const PARTS_PROBED: usize = super::wtf16::MAX_PARTS + 1;

impl<H: Host> Vm<H> {
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

    /// `StringConcat` of two or three parts, at most one a Number and the rest flat ASCII
    /// strings, whose text will not fit inline: a string of parts, which writes its text out
    /// when it is first read (`Heap::written_out_string`) and costs a record until then. `None`
    /// for any other parts.
    #[inline(always)]
    pub(super) fn string_of_parts(&mut self, count: usize, part: impl Fn(&Self, usize) -> Value) -> Option<Value> {
        let mut parts = smallvec::SmallVec::<[super::wtf16::Part; super::wtf16::MAX_PARTS]>::new();
        let (mut known, mut numbers) = (0, 0);
        for index in 0..count.min(PARTS_PROBED) {
            let value = part(self, index);
            match value.direct_string_index() {
                Some(record_id) => {
                    let text = self.heap.string(value)?;
                    if !text.existing_flat()?.is_ascii() {
                        return None;
                    }
                    known += text.len();
                    parts.push(super::wtf16::Part::Text(record_id));
                }
                None => {
                    numbers += 1;
                    parts.push(super::wtf16::Part::Number(value.as_number()?));
                }
            }
        }
        if known + numbers <= wtf16::SHORT_ASCII_CAPACITY || known >= PARTS_KNOWN_LIMIT {
            return None;
        }
        let parts = super::wtf16::Parts::new(&parts)?;
        Some(self.heap.alloc_string(JsString::from_parts(parts)))
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
        if let Some(string) = self.string_of_parts(count, &part) {
            return Ok(string);
        }
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
