use super::*;

/// The parts a `StringConcat` converts without a heap list: a template of up to four
/// substitutions and their text.
const STRING_CONCAT_INLINE_PARTS: usize = 9;
/// The most text a string of parts may hold in its known parts (regexp -3% at 256, flat at 64):
/// past it the copy a flat concatenation makes costs more than its record saves, and a string
/// read soon is copied twice.
const PARTS_KNOWN_LIMIT: usize = 64;
/// Direct-mapped entries of `ConcatMemo`.
const CONCAT_MEMO_ENTRIES: usize = 256;
/// The longest ASCII text a concatenation key packs into its word, beside its length byte.
const PACKED_TEXT_LIMIT: usize = 7;
/// The high bit of a packed key's length byte, which no integer's or atom's key sets there.
const PACKED_TEXT_MARK: u8 = 0x80;
/// The odd multiplier (2^64 / golden ratio) and right-key rotation spreading a key pair.
const CONCAT_MEMO_MIX: (u64, u32) = (0x9e37_79b9_7f4a_7c15, 17);

/// The atom `left + right` spells, by operand key (`Vm::concat_key`), which names each
/// operand's text. Atoms keep their text until the VM reassigns them, which clears it.
pub(super) struct ConcatMemo(Box<[(u64, u64, u32); CONCAT_MEMO_ENTRIES]>);

impl Default for ConcatMemo {
    fn default() -> Self {
        Self(Box::new([(u64::MAX, u64::MAX, 0); CONCAT_MEMO_ENTRIES]))
    }
}

impl ConcatMemo {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    #[inline(always)]
    fn index(left: u64, right: u64) -> usize {
        let (multiplier, rotation) = CONCAT_MEMO_MIX;
        let mixed = (left ^ right.rotate_left(rotation)).wrapping_mul(multiplier);
        (mixed >> (u64::BITS - CONCAT_MEMO_ENTRIES.trailing_zeros())) as usize
    }
}

impl<H: Host> Vm<H> {
    /// The memo key of a concatenation operand: an integer's bits, a short ASCII string's
    /// text and length packed into one word (high byte `0x80 | len`), or an atom string's atom
    /// (above the low word); `None` for any other value. Each key names its text alone.
    #[inline(always)]
    fn concat_key(&self, operand: Value) -> Option<u64> {
        if operand.as_int().is_some() {
            return Some(operand.to_bits());
        }
        if let Some(super::wtf16::Units::Ascii(text)) = self.heap.string(operand).and_then(|text| text.existing_flat()).map(|flat| flat.view())
            && text.len() <= PACKED_TEXT_LIMIT
        {
            let mut word = [0u8; PACKED_TEXT_LIMIT + 1];
            word[..text.len()].copy_from_slice(text);
            word[PACKED_TEXT_LIMIT] = PACKED_TEXT_MARK | text.len() as u8;
            return Some(u64::from_le_bytes(word));
        }
        self.heap.string_atom(operand).map(|atom| (1 << u32::BITS) | u64::from(atom))
    }

    /// The string of the atom `left + right` of a string and an integer spelled when it last
    /// ran (`ConcatMemo`), as `"k" + i` keys repeat; `None` for any other pair, or where the
    /// memo has no live string for it.
    #[inline(always)]
    pub(super) fn memoized_concat(&self, left: Value, right: Value) -> Option<Value> {
        // A key joins a string and an integer, as `"k" + i` does; two strings take the joins.
        if left.as_int().is_none() == right.as_int().is_none() {
            return None;
        }
        let (left, right) = (self.concat_key(left)?, self.concat_key(right)?);
        let (l, r, atom) = self.concat_memo.0[ConcatMemo::index(left, right)];
        if l != left || r != right {
            return None;
        }
        self.heap.atom_string(atom, false)
    }

    /// Records that `left + right` made the atom string `result` (`memoized_concat`).
    fn memoize_concat(&mut self, left: Value, right: Value, result: Value) {
        if left.as_int().is_none() == right.as_int().is_none() {
            return;
        }
        if let (Some(l), Some(r), Some(atom)) = (self.concat_key(left), self.concat_key(right), self.heap.string_atom(result)) {
            self.concat_memo.0[ConcatMemo::index(l, r)] = (l, r, atom);
        }
    }

    /// `left + right` of strings and Numbers whose text is ASCII and short enough to sit
    /// inline (`SHORT_ASCII_CAPACITY`), such as a key built as `"d" + i`: each operand's
    /// bytes written once into the result, with no string made per operand or joined.
    pub(super) fn short_ascii_concat(&mut self, left: Value, right: Value) -> Option<Value> {
        let mut bytes = crate::number_to_string::Short::new();
        for operand in [left, right] {
            // A small string's text never fits (`string_of_parts`): no view is made of it.
            if operand.direct_string_index().is_some_and(crate::heap::is_small_string) {
                return None;
            }
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
            self.memoize_concat(left, right, string);
            return Some(string);
        }
        let string = self.heap.alloc_string(JsString::from_str(text));
        atom.inspect(|atom| self.heap.record_string_atom(string, *atom));
        self.memoize_concat(left, right, string);
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
    /// for any other parts. It is a small string (`Heap::alloc_small_string`), in a smaller
    /// record, while that space has room.
    #[inline(always)]
    pub(super) fn string_of_parts(&mut self, count: usize, part: impl Fn(&Self, usize) -> Value) -> Option<Value> {
        use super::wtf16::{MAX_PARTS, MIN_PARTS, Parts};
        if !(MIN_PARTS..=MAX_PARTS).contains(&count) {
            return None;
        }
        // The string ids in order, and the Number and its place.
        let (mut ids, mut texts, mut number, mut known) = ([0; MAX_PARTS], 0, None, 0);
        for index in 0..count {
            let value = part(self, index);
            match value.direct_string_index() {
                Some(record_id) => {
                    known += self.heap.existing_ascii_len(value)?;
                    ids[texts] = record_id;
                    texts += 1;
                }
                None if number.is_none() => number = Some((value.as_number()?, index)),
                None => return None,
            }
        }
        let numbers = usize::from(number.is_some());
        if known + numbers <= wtf16::SHORT_ASCII_CAPACITY || known >= PARTS_KNOWN_LIMIT {
            return None;
        }
        let small = crate::heap::SmallParts::new(ids, texts, number).or_else(|| match number {
            // A Number between two strings that is no int32: the pair as a template.
            Some((value, 1)) if texts == 2 => {
                self.heap.small_template(ids[0], ids[1]).map(|template| crate::heap::SmallParts::templated(template, value))
            }
            _ => None,
        });
        if let Some(small) = small
            && let Some(string) = self.heap.alloc_small_string(small)
        {
            return Some(string);
        }
        let mut parts = Parts::EMPTY;
        let mut text = ids.into_iter();
        for index in 0..count {
            let pushed = match number {
                Some((value, at)) if at == index => parts.push_number(value),
                _ => parts.push_text(text.next().expect("a string id per string part")),
            };
            debug_assert!(pushed, "at most `MAX_PARTS` parts, one a Number");
        }
        Some(self.heap.alloc_parts_string(parts))
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
            Small(std::rc::Rc<str>),
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
                None => match self.heap.flat_text(value)? {
                    crate::heap::FlatText::Record(super::wtf16::Units::Ascii(bytes)) => Piece::Text(bytes),
                    crate::heap::FlatText::Record(super::wtf16::Units::Wide(_)) => return None,
                    crate::heap::FlatText::Small(text) => Piece::Small(text),
                },
            };
            length += match &piece {
                Piece::Text(bytes) => bytes.len(),
                Piece::Small(text) => text.len(),
                Piece::Number(text) => text.len(),
            };
            pieces.push(piece);
        }
        JsString::copies_whole(length).then(|| {
            JsString::from_ascii_parts(length, pieces.iter().map(|piece| match piece {
                Piece::Text(bytes) => *bytes,
                Piece::Small(text) => text.as_bytes(),
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
