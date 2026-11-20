use super::*;

/// The parts a `StringConcat` converts without a heap list: a template of up to four
/// substitutions and their text.
const STRING_CONCAT_INLINE_PARTS: usize = 9;
/// The ToString text of the immediate values a concatenation joins without a heap string.
const TRUE_TEXT: &[u8] = b"true";
const FALSE_TEXT: &[u8] = b"false";
const NULL_TEXT: &[u8] = b"null";
const UNDEFINED_TEXT: &[u8] = b"undefined";
/// The most text a string of parts may hold in its known parts (regexp -3% at 256, flat at 64):
/// past it the copy a flat concatenation makes costs more than its record saves, and a string
/// read soon is copied twice.
const PARTS_KNOWN_LIMIT: usize = 64;
/// Direct-mapped entries of `ConcatMemo`.
const CONCAT_MEMO_ENTRIES: usize = 256;
/// The odd multiplier (2^64 / golden ratio) and right-key rotation spreading a key pair.
const CONCAT_MEMO_MIX: (u64, u32) = (0x9e37_79b9_7f4a_7c15, 17);

/// The atom `left + right` spells, by operand key (`Vm::concat_key`), which names each
/// operand while the heap's collection epoch it records holds: a string's slot names the same
/// immutable string until a collection. Atoms keep their text until the VM reassigns them,
/// which clears it.
pub(super) struct ConcatMemo(Box<[(u64, u64, u32); CONCAT_MEMO_ENTRIES]>, u64);

impl Default for ConcatMemo {
    fn default() -> Self {
        Self(Box::new([(u64::MAX, u64::MAX, 0); CONCAT_MEMO_ENTRIES]), u64::MAX)
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
    /// The memo key of a concatenation operand: its value's bits, for an integer or a string,
    /// whose slot names it until a collection (`ConcatMemo`); `None` for any other value.
    #[inline(always)]
    fn concat_key(&self, operand: Value) -> Option<u64> {
        (operand.as_int().is_some() || self.heap.is_string(operand)).then(|| operand.to_bits())
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
        if l != left || r != right || self.concat_memo.1 != self.cache_epochs.gc {
            return None;
        }
        self.heap.atom_string(atom, false)
    }

    /// Records that `left + right` made the atom string `result` (`memoized_concat`).
    fn memoize_concat(&mut self, left: Value, right: Value, result: Value) {
        if left.as_int().is_none() == right.as_int().is_none() {
            return;
        }
        // Only an atom result is kept: a fresh text (`"id" + n` counting up) skips the keys.
        let Some(atom) = self.heap.string_atom(result) else {
            return;
        };
        if let (Some(l), Some(r)) = (self.concat_key(left), self.concat_key(right)) {
            let epoch = self.cache_epochs.gc;
            if self.concat_memo.1 != epoch {
                self.concat_memo.0.fill((u64::MAX, u64::MAX, 0));
                self.concat_memo.1 = epoch;
            }
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
                    super::wtf16::Units::Latin1(_) | super::wtf16::Units::Wide(_) => return None,
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
        // The inline string is made first: its hash reads its padded words, and neither it
        // nor the hash scans the bytes for ASCII again.
        let inline = JsString::from_ascii_bytes(&bytes);
        let hash = inline.inline_text_hash().expect("short ASCII text is held inline");
        let atom = self.find_atom(wtf16::table_hash(hash), |atom| self.atom_units_equal_str(atom, text));
        if let Some(string) = atom.and_then(|atom| self.heap.atom_string(atom, false)) {
            self.memoize_concat(left, right, string);
            return Some(string);
        }
        let string = self.heap.alloc_string(inline);
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

    /// `string_concat` of parts a string of parts does not take, where every part converts to a
    /// string without guest code (`converts_to_string_purely`); `None` for any other part or a
    /// result past the string length limit, which the general step makes or raises.
    pub(super) fn pure_string_concat_apart(&mut self, p: &ResidualProgram, count: usize,
        part: impl Fn(&Self, usize) -> Value + Copy) -> Option<Value> {
        // Each part is read once: the reads are pure, and both passes below use the values.
        let mut values = smallvec::SmallVec::<[Value; STRING_CONCAT_INLINE_PARTS]>::with_capacity(count);
        for index in 0..count {
            values.push(part(self, index));
        }
        self.pure_values_concat(p, &values)
    }

    /// `pure_string_concat_apart` of the part values `values`, already read: the ASCII join, else
    /// the converted join when every part converts to a string purely.
    fn pure_values_concat(&mut self, p: &ResidualProgram, values: &[Value]) -> Option<Value> {
        let part = |_: &Self, index: usize| values[index];
        if let Some(text) = self.ascii_string_concat(values.len(), part) {
            return Some(self.heap.alloc_string(text));
        }
        if !values.iter().all(|&value| self.converts_to_string_purely(value)) {
            return None;
        }
        self.converted_string_concat(p, values.len(), part).ok()
    }

    /// `string_concat` that runs no guest code: a string of parts, or `pure_string_concat_apart`.
    pub(super) fn pure_string_concat(&mut self, p: &ResidualProgram, count: usize,
        part: impl Fn(&Self, usize) -> Value + Copy) -> Option<Value> {
        self.string_of_parts(count, part).or_else(|| self.pure_string_concat_apart(p, count, part))
    }

    /// `StringConcat` of the register `window` of frame `f`.
    pub(super) fn frame_string_concat(&mut self, p: &ResidualProgram, f: usize, window: crate::bytecode::RegisterWindow) -> Result<Value, JsError> {
        let start = usize::from(window.base);
        self.string_concat(p, usize::from(window.count), |vm, part| vm.frames[f].registers[start + part])
    }

    /// `string_concat` of parts that are numbers, booleans, null, undefined and flat ASCII
    /// strings, short together: the total length is measured first (`ascii_part_len`), then each
    /// part is written once straight into the result's storage (`JsString::from_ascii_filled`).
    /// `None` for any other part, which `converted_string_concat` converts, or for a result past
    /// `STRING_CONCAT_MAX_BYTES`.
    #[inline(always)]
    pub(super) fn ascii_string_concat(&self, count: usize, part: impl Fn(&Self, usize) -> Value) -> Option<JsString> {
        // A Number part is formatted here once, and its text is written from this list.
        let mut texts =
            smallvec::SmallVec::<[Option<crate::number_to_string::Short>; STRING_CONCAT_INLINE_PARTS]>::with_capacity(count);
        let mut length = 0;
        for index in 0..count {
            let value = part(self, index);
            let text = number_part_text(value);
            length += match &text {
                Some(text) => text.len(),
                None => self.ascii_part_len(value)?,
            };
            if length > STRING_CONCAT_MAX_BYTES {
                return None;
            }
            texts.push(text);
        }
        Some(JsString::from_ascii_filled(length, |out| {
            let mut at = 0;
            for (index, text) in texts.iter().enumerate() {
                at = match text {
                    Some(text) => put_text(out, at, text),
                    None => self.write_ascii_part(part(self, index), out, at),
                };
            }
            assert_eq!(at, out.len(), "the parts fill the length measured for them");
        }))
    }

    /// The byte length `write_ascii_part` writes for the part `value`, which is not a Number;
    /// `None` for a part that is not flat ASCII text, which the concatenation does not join.
    #[inline(always)]
    fn ascii_part_len(&self, value: Value) -> Option<usize> {
        if let Some(boolean) = value.as_bool() {
            return Some(if boolean { TRUE_TEXT.len() } else { FALSE_TEXT.len() });
        }
        if value.is_null() {
            return Some(NULL_TEXT.len());
        }
        if value.is_undefined() {
            return Some(UNDEFINED_TEXT.len());
        }
        match self.heap.flat_text(value)? {
            crate::heap::FlatText::Record(super::wtf16::Units::Ascii(bytes)) => Some(bytes.len()),
            crate::heap::FlatText::Record(super::wtf16::Units::Wide(_) | super::wtf16::Units::Latin1(_)) => None,
            crate::heap::FlatText::Small(small) => Some(small.as_bytes().len()),
        }
    }

    /// Writes the text of the part `value`, which is not a Number, into `out` at `at`, returning
    /// the offset past it. The part was measured by `ascii_part_len`, so it is flat ASCII text.
    #[inline(always)]
    fn write_ascii_part(&self, value: Value, out: &mut [std::mem::MaybeUninit<u8>], at: usize) -> usize {
        if let Some(boolean) = value.as_bool() {
            return put_text(out, at, if boolean { TRUE_TEXT } else { FALSE_TEXT });
        }
        if value.is_null() {
            return put_text(out, at, NULL_TEXT);
        }
        if value.is_undefined() {
            return put_text(out, at, UNDEFINED_TEXT);
        }
        match self.heap.flat_text(value) {
            Some(crate::heap::FlatText::Record(super::wtf16::Units::Ascii(bytes))) => put_text(out, at, bytes),
            Some(crate::heap::FlatText::Small(small)) => put_text(out, at, small.as_bytes()),
            _ => unreachable!("`ascii_part_len` measured this part as flat ASCII"),
        }
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

/// The longest result `ascii_string_concat` makes: the longest text a string copies whole
/// (`JsString::copies_whole`); a longer one is a rope of its parts.
const STRING_CONCAT_MAX_BYTES: usize = 512;

/// The text of a Number part, an int32 or a double, held on the stack; `None` for any other part.
#[inline(always)]
fn number_part_text(value: Value) -> Option<crate::number_to_string::Short> {
    if let Some(int) = value.as_int() {
        let mut text = crate::number_to_string::Short::new();
        text.try_extend(itoa::Buffer::new().format(int).as_bytes()).expect("an int32's decimal text fits a Short");
        return Some(text);
    }
    value.as_number().map(number_text)
}

/// Number::toString(10) of `number` as ASCII text, held on the stack.
#[inline(always)]
fn number_text(number: f64) -> crate::number_to_string::Short {
    let mut text = crate::number_to_string::Short::new();
    crate::number_to_string::write(number, &mut text);
    text
}

/// Copies `bytes` into `out` at `at`, returning the offset past them.
#[inline(always)]
fn put_text(out: &mut [std::mem::MaybeUninit<u8>], at: usize, bytes: &[u8]) -> usize {
    let target = &mut out[at..at + bytes.len()];
    // SAFETY: `target` has exactly `bytes.len()` slots, which `bytes` cannot overlap (it is a
    // borrow of the heap, a constant or a formatting buffer, never of `out`); `u8` and
    // `MaybeUninit<u8>` share a layout, so the copy initializes the slots.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.as_mut_ptr().cast::<u8>(), bytes.len()) };
    at + bytes.len()
}
