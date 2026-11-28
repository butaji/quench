use super::object_descriptors::PropertyDescriptorRecord;
use super::atom_keys::AtomText;
use super::wtf16::{JsString, Units};
use super::number_parse::{decimal_value, exact_integer, scan_decimal};
use super::*;
use crate::{number_to_string::AppendBytes as _, unicode};
use std::hash::BuildHasher as _;
use std::io::Write as _;
use std::ops::Range;

mod heap_builder;
mod error_message;
mod plain_write;
mod parse_stacks;
mod byte_class;
use byte_class::{byte_class, byte_class_of, exponent_mark, JsonByteClass};
#[cfg(target_arch = "x86_64")]
use byte_class::WHITESPACE_BYTES;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{
    __m128i, _mm_and_si128, _mm_cmpeq_epi16, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_min_epu8, _mm_movemask_epi8,
    _mm_or_si128, _mm_packs_epi16, _mm_set1_epi16, _mm_set1_epi8, _mm_setzero_si128,
};
use error_message::json_syntax_message;
use heap_builder::{short_run_ends, JsonBuildError, JsonHeapBuilder, JsonRevivalBuilder, JsonSnapshot};
use parse_stacks::{JsonFrame, JsonStacks};
pub(super) use heap_builder::JsonParseCache;

const JSON_NULL_INITIAL: u16 = b'n' as u16;
const JSON_TRUE_INITIAL: u16 = b't' as u16;
const JSON_FALSE_INITIAL: u16 = b'f' as u16;
const JSON_QUOTE: u16 = b'"' as u16;
const JSON_ESCAPE: u16 = b'\\' as u16;
const JSON_OBJECT_END: u16 = b'}' as u16;
const JSON_ARRAY_END: u16 = b']' as u16;
const JSON_COMMA: u16 = b',' as u16;
const JSON_COLON: u16 = b':' as u16;
const JSON_MINUS: u16 = b'-' as u16;
const JSON_DIGIT_START: u16 = b'0' as u16;
const JSON_DIGIT_END: u16 = b'9' as u16;
const JSON_UNESCAPED_START: u16 = b' ' as u16;
const JSON_HEX_ESCAPE_DIGITS: usize = 4;
const JSON_HEX_LETTER_VALUE_START: u16 = 10;
const JSON_BACKSPACE: u16 = b'\x08' as u16;
const JSON_FORM_FEED: u16 = b'\x0C' as u16;
/// Whether byte runs `a` and `b` are equal, compared inline a word at a time: the runs a
/// parse compares, keys and repeated strings, are mostly a few words long.
#[inline(always)]
fn same_ascii(a: &[u8], b: &[u8]) -> bool {
    let length = a.len();
    if length != b.len() {
        return false;
    }
    if length < WORD_BYTES {
        return short_run_ends(a) == short_run_ends(b);
    }
    let (a_words, a_tail) = a.as_chunks::<WORD_BYTES>();
    let (b_words, _) = b.as_chunks::<WORD_BYTES>();
    // A partial last word is compared as the word that ends the text, overlapping the one
    // before it.
    a_words.iter().zip(b_words).all(|(a_word, b_word)| a_word == b_word)
        && (a_tail.is_empty() || a.last_chunk::<WORD_BYTES>() == b.last_chunk::<WORD_BYTES>())
}

/// Bytes in one SIMD block: an SSE2 register, which the byte scans read whole.
const SIMD_BLOCK_BYTES: usize = 16;

/// The bytes of `block` in one SSE2 register, read by an unaligned load.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn sse2_load(block: &[u8; SIMD_BLOCK_BYTES]) -> __m128i {
    // SAFETY: SSE2 is part of the x86_64 baseline, and the load reads exactly the block's bytes.
    unsafe { _mm_loadu_si128(block.as_ptr().cast()) }
}

/// The mask of the bytes of `block` that end a string run (`ends_string_run`): bit `i` is set
/// when byte `i` is a quote, an escape or a control byte.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn sse2_run_end_mask(block: __m128i) -> u32 {
    // SAFETY: SSE2 is part of the x86_64 baseline, and these intrinsics touch no memory.
    unsafe {
        // A byte is a control byte exactly when its unsigned minimum with the last one is itself.
        let control = _mm_cmpeq_epi8(_mm_min_epu8(block, _mm_set1_epi8(LAST_CONTROL_CHARACTER as i8)), block);
        let quote = _mm_cmpeq_epi8(block, _mm_set1_epi8(JSON_QUOTE as i8));
        let escape = _mm_cmpeq_epi8(block, _mm_set1_epi8(JSON_ESCAPE as i8));
        _mm_movemask_epi8(_mm_or_si128(_mm_or_si128(quote, escape), control)) as u32
    }
}

/// The mask of the bytes of `block` that are JSON whitespace: bit `i` is set when byte `i` is one.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn sse2_whitespace_mask(block: __m128i) -> u32 {
    // SAFETY: SSE2 is part of the x86_64 baseline, and these intrinsics touch no memory.
    unsafe {
        let mut whitespace = _mm_setzero_si128();
        for byte in WHITESPACE_BYTES {
            whitespace = _mm_or_si128(whitespace, _mm_cmpeq_epi8(block, _mm_set1_epi8(byte as i8)));
        }
        _mm_movemask_epi8(whitespace) as u32
    }
}

/// The length of the run of `units` no byte ends (`ends_string_run`), from whole blocks: `Ok`
/// of the offset of the first ending byte, else `Err` of the bytes the blocks covered.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn sse2_run_blocks(units: &[u8]) -> Result<usize, usize> {
    let (blocks, _) = units.as_chunks::<SIMD_BLOCK_BYTES>();
    for (index, block) in blocks.iter().enumerate() {
        let mask = sse2_run_end_mask(sse2_load(block));
        if mask != 0 {
            return Ok(index * SIMD_BLOCK_BYTES + mask.trailing_zeros() as usize);
        }
    }
    Err(blocks.len() * SIMD_BLOCK_BYTES)
}

/// `sse2_run_blocks` of the tail after its whole blocks, which is shorter than a block. The
/// tail is padded with quotes, which end a run: an end at or past the tail's length is no end
/// of the tail's own bytes.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn sse2_run_tail(tail: &[u8]) -> Option<usize> {
    let mut block = [JSON_QUOTE as u8; SIMD_BLOCK_BYTES];
    block[..tail.len()].copy_from_slice(tail);
    let position = sse2_run_end_mask(sse2_load(&block)).trailing_zeros() as usize;
    (position < tail.len()).then_some(position)
}

/// Units in one SSE2 block of 16-bit units: a register of eight lanes.
#[cfg(target_arch = "x86_64")]
const WIDE_BLOCK_UNITS: usize = 8;

/// The bits of a wide block's mask: one per unit of the block.
#[cfg(target_arch = "x86_64")]
const WIDE_BLOCK_MASK: u32 = (1 << WIDE_BLOCK_UNITS) - 1;

/// The mask of the units of a wide block that end a string run (`ends_string_run`): bit `i`
/// is set when unit `i` is a quote, an escape or a control unit. A unit is a control unit
/// exactly when none of its bits past the control range is set.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn sse2_wide_run_end_mask(block: &[u16; WIDE_BLOCK_UNITS]) -> u32 {
    // SAFETY: SSE2 is part of the x86_64 baseline, and the load reads exactly the block's units.
    unsafe {
        let units = _mm_loadu_si128(block.as_ptr().cast());
        let quote = _mm_cmpeq_epi16(units, _mm_set1_epi16(JSON_QUOTE as i16));
        let escape = _mm_cmpeq_epi16(units, _mm_set1_epi16(JSON_ESCAPE as i16));
        let control_bits = _mm_set1_epi16(!(LAST_CONTROL_CHARACTER as i16));
        let control = _mm_cmpeq_epi16(_mm_and_si128(units, control_bits), _mm_setzero_si128());
        let ends = _mm_or_si128(_mm_or_si128(quote, escape), control);
        // Each lane is all ones or all zeros, which saturating packs keeps; the second copy
        // of the lanes is masked off.
        let packed = _mm_packs_epi16(ends, ends);
        (_mm_movemask_epi8(packed) as u32) & WIDE_BLOCK_MASK
    }
}

/// `sse2_run_blocks` of 16-bit units, a block of eight at a time.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn sse2_wide_run_blocks(units: &[u16]) -> Result<usize, usize> {
    let (blocks, _) = units.as_chunks::<WIDE_BLOCK_UNITS>();
    for (index, block) in blocks.iter().enumerate() {
        let mask = sse2_wide_run_end_mask(block);
        if mask != 0 {
            return Ok(index * WIDE_BLOCK_UNITS + mask.trailing_zeros() as usize);
        }
    }
    Err(blocks.len() * WIDE_BLOCK_UNITS)
}

/// `sse2_run_tail` of 16-bit units: the tail is padded with quotes, which end a run.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn sse2_wide_run_tail(tail: &[u16]) -> Option<usize> {
    let mut block = [JSON_QUOTE; WIDE_BLOCK_UNITS];
    block[..tail.len()].copy_from_slice(tail);
    let position = sse2_wide_run_end_mask(&block).trailing_zeros() as usize;
    (position < tail.len()).then_some(position)
}

/// The whitespace bytes at the start of `units` in whole blocks: the count stops in the block
/// that holds the first byte which is not whitespace, or at the last whole block.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn sse2_whitespace_blocks(units: &[u8]) -> usize {
    let (blocks, _) = units.as_chunks::<SIMD_BLOCK_BYTES>();
    for (index, block) in blocks.iter().enumerate() {
        // Whitespace bytes are the set bits of the mask, so the first clear bit ends the run.
        let run = (!sse2_whitespace_mask(sse2_load(block))).trailing_zeros() as usize;
        if run < SIMD_BLOCK_BYTES {
            return index * SIMD_BLOCK_BYTES + run;
        }
    }
    blocks.len() * SIMD_BLOCK_BYTES
}

/// The whitespace bytes at the start of `units`, one table load per byte.
#[inline(always)]
fn scalar_whitespace_run(units: &[u8]) -> usize {
    units.iter().take_while(|&&byte| byte_class_of(byte) == JsonByteClass::Whitespace).count()
}

/// `string_run_length` of bytes a word at a time, for targets without SSE2 blocks. The tail is
/// padded with quotes to a word, as `sse2_run_tail` pads to a block.
#[cfg(not(target_arch = "x86_64"))]
fn word_run_length(units: &[u8]) -> Option<usize> {
    let (words, tail) = units.as_chunks::<WORD_BYTES>();
    for (index, word) in words.iter().enumerate() {
        let ends = json_escape_bytes(u64::from_le_bytes(*word));
        if ends != 0 {
            return Some(index * WORD_BYTES + (ends.trailing_zeros() / u8::BITS) as usize);
        }
    }
    let mut padded = [JSON_QUOTE as u8; WORD_BYTES];
    padded[..tail.len()].copy_from_slice(tail);
    let position = (json_escape_bytes(u64::from_le_bytes(padded)).trailing_zeros() / u8::BITS) as usize;
    (position < tail.len()).then_some(words.len() * WORD_BYTES + position)
}

/// The containers being written, inline up to this nesting.
const JSON_PLAIN_INLINE_ANCESTORS: usize = 16;

/// The bytes of a member's key text (`JsonMember::key`) before its quoted key: the comma.
const JSON_KEY_TEXT_COMMA: usize = 1;

/// JSON.stringify's arguments past the value, as its steps read them: the replacer function,
/// the property list of a replacer array, and the gap. The steady call passes none.
#[derive(Default)]
struct JsonOptions {
    replacer: Option<Value>,
    property_list: Option<Vec<Atom>>,
    gap: String,
}

/// Why the writer stopped: the steady call met a value that needs the specification's steps,
/// having observed nothing (`Plain`), or a step threw.
enum JsonStop {
    Plain,
    Thrown(JsError),
}

impl From<JsError> for JsonStop {
    fn from(error: JsError) -> Self {
        Self::Thrown(error)
    }
}

/// What SerializeJSONProperty did with a value: wrote it, or omitted it (`false`).
type JsonStep = Result<bool, JsonStop>;

/// The state of one stringification. What a shape and prototype decide (lacking `toJSON`, an
/// object's members) holds while the collection epoch it was decided under holds and the chain
/// cell of its prototypes holds (`JsonPlanProof`); the specification's steps report each place
/// guest code may have run (`JsonWriter::after_guest_code`).
struct JsonWriter {
    to_json: Atom,
    /// The root value's key in its wrapper object.
    empty: Atom,
    options: JsonOptions,
    /// The containers being written: the cycle test, and the gap's indentation depth.
    ancestors: smallvec::SmallVec<[Value; JSON_PLAIN_INLINE_ANCESTORS]>,
    output: JsonText,
    /// The collection epoch the plans were proved under.
    epoch: u64,
    /// Counts the places guest code may have run: a plan or a dense array proved under one
    /// count describes its object while the count holds.
    steps: u64,
    /// That the array (shape, prototype) lacks `toJSON`, and the chain cell of its prototype
    /// it was proved over.
    array_lacks_to_json: Option<((u32, Value), crate::heap::ChainCell)>,
    /// The member plan of each ordinary object's shape, indexed by the shape id (a dense index
    /// into the shape table), with the prototype the plan was proved for: an object of the
    /// shape under another prototype is planned afresh, and replaces the entry.
    plans: Vec<Option<(Value, JsonPlanProof)>>,
    /// The `,"key":` text of every planned member: an object's keys recur across the objects
    /// of a payload, so each shape's are quoted once. Only a new run clears it, so the plans a
    /// run is writing keep their text when the proofs are dropped.
    key_text: JsonText,
}

/// One enumerable data member of a planned object: its key, its slot, and its `,"key":`
/// text's range in `JsonWriter::key_text`, whose first byte, the comma, a first written
/// member skips.
#[derive(Clone, Copy)]
struct JsonMember {
    atom: Atom,
    slot: u32,
    key: (u32, u32),
}

/// The members of an object being written, fixed as it is entered (SerializeJSONObject's K):
/// its plan's, whose slots hold their values while `steps` holds, or keys whose values are
/// read by key.
enum JsonMembers {
    Planned { plan: std::rc::Rc<[JsonMember]>, steps: u64 },
    Keys(super::object_keys::EnumerableKeys<Atom>),
}

impl JsonMembers {
    fn len(&self) -> usize {
        match self {
            Self::Planned { plan, .. } => plan.len(),
            Self::Keys(keys) => keys.len(),
        }
    }
}

/// What stringifications keep between calls (`Vm::json_plain`): the writer, with its text
/// buffer and the member plans of the (shape, prototype) pairs met, as V8 keeps a map's "fast
/// serializable" bit. A plan is a fact of a shape and a prototype chain, so it holds while the
/// collection epoch (shape ids, prototype records) stays where it was when it was made, and the
/// chain cell of its prototypes (`JsonPlanProof`) holds: a prototype gaining `toJSON` or an
/// accessor retires that cell alone.
#[derive(Default)]
pub(crate) struct JsonPlainCache {
    /// Boxed so a call moves the state out and back as one pointer; a nested call (from a
    /// `toJSON` or replacer) finds none and makes its own.
    write: Option<Box<JsonWriter>>,
}

/// A member plan (or its refusal, `None`) proved for one (shape, prototype) pair, held while
/// the chain cell of the prototype it read holds (`JsonWriter::plans`).
#[derive(Clone)]
struct JsonPlanProof {
    cell: crate::heap::ChainCell,
    plan: Option<std::rc::Rc<[JsonMember]>>,
}

impl JsonWriter {
    fn new(to_json: Atom, empty: Atom, epoch: u64) -> Self {
        Self {
            to_json,
            empty,
            options: JsonOptions::default(),
            ancestors: Default::default(),
            output: JsonText::new(),
            epoch,
            steps: 0,
            array_lacks_to_json: None,
            plans: Default::default(),
            key_text: JsonText::new(),
        }
    }

    /// Forgets every proof, keeping the plans being written their key text.
    fn forget_proofs(&mut self) {
        self.array_lacks_to_json = None;
        self.plans.clear();
    }

    /// Forgets every plan and its key text, between runs.
    fn forget_plans(&mut self) {
        self.forget_proofs();
        self.key_text.clear();
    }

    /// Starts a run under `epoch`: plans proved under another collection epoch are forgotten.
    fn prove(&mut self, epoch: u64) {
        if self.epoch != epoch {
            self.forget_plans();
            self.epoch = epoch;
        }
    }

    /// Records that guest code may have run, leaving `epoch`: no planned slot or dense element
    /// proved before is read again, and plans proved under another collection epoch are dropped.
    fn after_guest_code(&mut self, epoch: u64) {
        self.steps += 1;
        if self.epoch != epoch {
            self.forget_proofs();
            self.epoch = epoch;
        }
    }

    /// Starts a member of the innermost container, which already holds `written` members:
    /// the separator, then the line break and indentation a gap asks for.
    fn begin_member(&mut self, written: usize) {
        if written != 0 {
            self.output.push(b',');
        }
        if !self.options.gap.is_empty() {
            self.output.push(b'\n');
            write_indent(&mut self.output, &self.options.gap, self.ancestors.len());
        }
    }

    /// `begin_member` and the planned member's quoted key and colon, from its key text.
    fn begin_planned_member(&mut self, member: JsonMember, written: usize) {
        let (start, end) = (member.key.0 as usize, member.key.1 as usize);
        if self.options.gap.is_empty() {
            let start = start + if written == 0 { JSON_KEY_TEXT_COMMA } else { 0 };
            self.output.extend_from_slice(&self.key_text[start..end]);
        } else {
            self.begin_member(written);
            self.output.extend_from_slice(&self.key_text[start + JSON_KEY_TEXT_COMMA..end]);
            self.output.push(b' ');
        }
    }

    /// The text between a member's key and its value.
    fn write_colon(&mut self) {
        self.output.extend_from_slice(if self.options.gap.is_empty() { b":" } else { b": " });
    }

    /// Closes the innermost container of `written` members with `close`.
    fn end_container(&mut self, written: usize, close: u8) {
        if written != 0 && !self.options.gap.is_empty() {
            self.output.push(b'\n');
            write_indent(&mut self.output, &self.options.gap, self.ancestors.len() - 1);
        }
        self.output.push(close);
    }
}

/// `JSON.stringify(value, replacer, space)`'s argument positions.
const JSON_REPLACER_ARGUMENT: usize = 1;
const JSON_SPACE_ARGUMENT: usize = 2;

/// SerializeJSONValue's TypeError for a BigInt.
const JSON_BIGINT_MESSAGE: &str = "Do not know how to serialize a BigInt";

/// The key SerializeJSONProperty reads from its holder: an array index, or a property name.
/// Its string value is allocated only for the `toJSON` and replacer calls that receive it.
#[derive(Clone, Copy)]
enum JsonKey {
    Index(usize),
    Name(Atom),
}

/// An integer's decimal digits, as Number::toString writes them.
fn write_json_integer(value: i32, output: &mut JsonText) {
    output.append_bytes(itoa::Buffer::new().format(value).as_bytes());
}

/// The JSON text a stringification writes: UTF-8 bytes, decoded once when it completes.
type JsonText = Vec<u8>;

/// A completed JSON text as a string. Every writer appends UTF-8: ASCII punctuation,
/// literals and numbers, host strings, and JavaScript strings whose lone surrogates it
/// escapes (`write_json_string`).
fn json_text_str(text: &[u8]) -> &str {
    debug_assert!(std::str::from_utf8(text).is_ok(), "JSON text is written as UTF-8");
    // SAFETY: as above, every byte sequence written is UTF-8.
    unsafe { std::str::from_utf8_unchecked(text) }
}

fn write_json_string(value: &JsString, output: &mut JsonText) {
    match value.units() {
        Units::Ascii(bytes) => write_json_utf8(bytes, output),
        Units::Latin1(bytes) => write_json_latin1(bytes, output),
        Units::Wide(units) => write_json_units(units, output),
    }
}

/// QuoteJSONString of Latin-1 `bytes`, one unit per byte, without widening them. An ASCII
/// run is `write_json_utf8`'s text; a byte past ASCII is the character it is, written as
/// its UTF-8 bytes. Every byte is escaped by the one rule of `json_escape_needed` and
/// `write_json_scalar`, as in `write_json_utf8`.
fn write_json_latin1(bytes: &[u8], output: &mut JsonText) {
    if bytes.is_ascii() {
        write_json_utf8(bytes, output);
        return;
    }
    output.push(b'"');
    let mut run = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if byte >= 0x80 || json_escape_needed(byte) {
            output.append_bytes(&bytes[run..index]);
            write_json_scalar(char::from(byte), output);
            run = index + 1;
        }
    }
    output.append_bytes(&bytes[run..]);
    output.push(b'"');
}

/// QuoteJSONString of UTF-16 `units`, each lone surrogate escaped as `\uXXXX`.
fn write_json_units(units: &[u16], output: &mut JsonText) {
    output.push(b'"');
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        let scalar = if let Some(code_point) = units
            .get(index + 1)
            .and_then(|low| unicode::decode_surrogate_pair(unit, *low))
        {
            index += 2;
            Some(char::from_u32(code_point).expect("valid surrogate pair"))
        } else {
            index += 1;
            char::from_u32(u32::from(unit))
        };
        match scalar {
            Some(scalar) => write_json_scalar(scalar, output),
            None => write_unicode_escape(u32::from(unit), output),
        }
    }
    output.push(b'"');
}

/// QuoteJSONString of UTF-8 `text`. Every byte QuoteJSONString escapes is ASCII and no
/// multi-byte sequence contains one, so the runs between escapes are copied whole.
fn write_json_utf8(text: &[u8], output: &mut JsonText) {
    output.reserve(text.len() + 2);
    // Most strings need no escape: one test of their words, then one copy.
    if !text_needs_json_escape(text) {
        output.push(b'"');
        output.extend_from_slice(text);
        output.push(b'"');
        return;
    }
    output.push(b'"');
    let mut run = 0;
    let mut index = 0;
    while index < text.len() {
        // A word of bytes none of which needs an escape is skipped whole.
        if let Some(word) = text.get(index..index + WORD_BYTES)
            && !word_needs_json_escape(u64::from_le_bytes(word.try_into().expect("word bytes")))
        {
            index += WORD_BYTES;
            continue;
        }
        let end = (index + WORD_BYTES).min(text.len());
        for (offset, byte) in text[index..end].iter().enumerate() {
            if json_escape_needed(*byte) {
                output.append_bytes(&text[run..index + offset]);
                write_json_scalar(char::from(*byte), output);
                run = index + offset + 1;
            }
        }
        index = end;
    }
    output.append_bytes(&text[run..]);
    output.push(b'"');
}

/// Whether any byte of `text` needs an escape, by words: the last partial word is padded
/// with spaces, which need none.
#[inline(always)]
fn text_needs_json_escape(text: &[u8]) -> bool {
    let mut words = text.chunks_exact(WORD_BYTES);
    for word in &mut words {
        if word_needs_json_escape(u64::from_le_bytes(word.try_into().expect("word bytes"))) {
            return true;
        }
    }
    let rest = words.remainder();
    let mut padded = [b' '; WORD_BYTES];
    padded[..rest.len()].copy_from_slice(rest);
    word_needs_json_escape(u64::from_le_bytes(padded))
}

/// Bytes scanned at once by `word_needs_json_escape`.
const WORD_BYTES: usize = size_of::<u64>();
/// One in each byte of a word, and each byte's high bit.
const BYTE_ONES: u64 = u64::MAX / u8::MAX as u64;
const BYTE_HIGHS: u64 = BYTE_ONES << (u8::BITS - 1);

/// Whether any byte of `word` needs an escape (`json_escape_needed`): a byte below
/// `LAST_CONTROL_CHARACTER + 1`, or equal to a quote or backslash. Each test borrows into
/// a byte's high bit only from such a byte or from a lower one that already passed, so the
/// word test is exact even though its per-byte bits are not.
fn word_needs_json_escape(word: u64) -> bool {
    json_escape_bytes(word) != 0
}

/// The high bit of the bytes of `word` that need an escape (`json_escape_needed`), exact for
/// the lowest of them: a borrow reaches a byte's high bit only from a lower byte that needs
/// one, so only bytes above the lowest can be marked without needing one.
#[inline(always)]
fn json_escape_bytes(word: u64) -> u64 {
    let below = |bound: u8| word.wrapping_sub(BYTE_ONES * u64::from(bound)) & !word & BYTE_HIGHS;
    let equal = |byte: u8| {
        let difference = word ^ (BYTE_ONES * u64::from(byte));
        difference.wrapping_sub(BYTE_ONES) & !difference & BYTE_HIGHS
    };
    below(LAST_CONTROL_CHARACTER + 1) | equal(b'"') | equal(b'\\')
}

/// QuoteJSONString escapes every code point up to U+001F.
const LAST_CONTROL_CHARACTER: u8 = 0x1f;

/// Whether QuoteJSONString writes the ASCII `byte` as an escape rather than as itself.
fn json_escape_needed(byte: u8) -> bool {
    matches!(byte, b'"' | b'\\') || byte <= LAST_CONTROL_CHARACTER
}

fn write_json_scalar(scalar: char, output: &mut JsonText) {
    let escape: &[u8] = match scalar {
        '"' => b"\\\"",
        '\\' => b"\\\\",
        '\u{08}' => b"\\b",
        '\u{0c}' => b"\\f",
        '\n' => b"\\n",
        '\r' => b"\\r",
        '\t' => b"\\t",
        scalar if u32::from(scalar) <= u32::from(LAST_CONTROL_CHARACTER) => {
            return write_unicode_escape(u32::from(scalar), output);
        }
        scalar => {
            let mut buffer = [0; char::MAX_LEN_UTF8];
            return output.append_bytes(scalar.encode_utf8(&mut buffer).as_bytes());
        }
    };
    output.append_bytes(escape);
}

/// The `\uXXXX` escape of `code`.
fn write_unicode_escape(code: u32, output: &mut JsonText) {
    let _ = write!(output, "\\u{code:04x}");
}

fn write_indent(output: &mut JsonText, gap: &str, depth: usize) {
    for _ in 0..depth {
        output.append_bytes(gap.as_bytes());
    }
}

/// A JSON syntax error and the unit it reports, an index into the parsed units.
#[derive(Debug)]
struct JsonParseError {
    syntax: JsonSyntax,
    position: usize,
}

/// A JSON syntax error as V8's `JsonParser` reports it (`MessageTemplate::kJsonParse*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsonSyntax {
    /// The unit reported is not a token the grammar allows there; its message follows from
    /// the unit (`JsonParser::ReportUnexpectedCharacter`).
    Unexpected,
    UnterminatedString,
    ExpectedPropertyNameOrBrace,
    ExpectedCommaOrBracket,
    ExpectedCommaOrBrace,
    ExpectedDoubleQuotedPropertyName,
    ExpectedColon,
    ExponentMissingNumber,
    UnterminatedFraction,
    NonWhitespaceAfter,
    /// The unit after a backslash escapes nothing; past Latin-1 it is `Unexpected`.
    BadEscape,
    BadControlCharacter,
    BadUnicodeEscape,
    NoNumberAfterMinus,
}

impl JsonSyntax {
    /// The message of an error V8 reports with its position, which follows the message.
    fn positioned_message(self) -> &'static str {
        match self {
            Self::Unexpected => unreachable!("an unexpected unit's message follows from the unit"),
            Self::UnterminatedString => "Unterminated string in JSON",
            Self::ExpectedPropertyNameOrBrace => "Expected property name or '}' in JSON",
            Self::ExpectedCommaOrBracket => "Expected ',' or ']' after array element in JSON",
            Self::ExpectedCommaOrBrace => "Expected ',' or '}' after property value in JSON",
            Self::ExpectedDoubleQuotedPropertyName => "Expected double-quoted property name in JSON",
            Self::ExpectedColon => "Expected ':' after property name in JSON",
            Self::ExponentMissingNumber => "Exponent part is missing a number in JSON",
            Self::UnterminatedFraction => "Unterminated fractional number in JSON",
            Self::NonWhitespaceAfter => "Unexpected non-whitespace character after JSON",
            Self::BadEscape => "Bad escaped character in JSON",
            Self::BadControlCharacter => "Bad control character in string literal in JSON",
            Self::BadUnicodeEscape => "Bad Unicode escape in JSON",
            Self::NoNumberAfterMinus => "No number after minus sign in JSON",
        }
    }
}

/// V8's message for JSON.rawJSON text that is not one whole primitive
/// (`MessageTemplate::kInvalidRawJsonValue`).
const JSON_INVALID_RAW_TEXT: &str = "Invalid value for JSON.rawJSON";

use crate::units::{CARRIAGE_RETURN, DECIMAL_RADIX, LINE_FEED};

/// Number literals at most this long parse from a stack buffer.
const JSON_INLINE_NUMBER_LENGTH: usize = 64;
/// Digits a number literal without an exponent holds to be valued in place (`short_number`): at
/// most 15 digits keep the mantissa below 10^15, exact in a double.
const JSON_SHORT_NUMBER_DIGITS: usize = 15;
/// The exact powers of ten a short fraction divides its mantissa by, 10^0 through 10^15.
const JSON_SHORT_POWERS_OF_TEN: [f64; JSON_SHORT_NUMBER_DIGITS + 1] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
];

/// A number literal's value: an `i32` when it is one, which needs no float conversion.
enum JsonNumber {
    Integer(i32),
    Float(f64),
}

/// Whether a string's unescaped run ends at `unit`: its closing quote, an escape, or a
/// control unit, which a JSON string may not hold unescaped.
#[inline(always)]
const fn ends_string_run(unit: u16) -> bool {
    unit == JSON_QUOTE || unit == JSON_ESCAPE || unit < JSON_UNESCAPED_START
}

/// `ends_string_run` of every byte, so a byte source's run is scanned one load per unit.
const STRING_RUN_ENDS: [bool; 1 << u8::BITS] = {
    let mut ends = [false; 1 << u8::BITS];
    let mut unit = 0;
    while unit < ends.len() {
        ends[unit] = ends_string_run(unit as u16);
        unit += 1;
    }
    ends
};

/// A code unit a JSON text is scanned in: a byte of an ASCII string, which is its UTF-16
/// unit, a UTF-16 unit, or a byte of UTF-8 text (`Utf8Byte`).
trait JsonUnit: Copy {
    /// The unit the structure reads: JSON's punctuation, literals, numbers and whitespace
    /// are ASCII, which every form spells as its UTF-16 unit.
    fn unit(self) -> u16;
    /// `ends_string_run` of this unit.
    #[inline(always)]
    fn ends_string_run(self) -> bool {
        ends_string_run(self.unit())
    }
    /// The units of `units` before the first that `ends_string_run`; `None` when none does. A
    /// length returned is the index of the ending unit, which lies within `units`.
    #[inline(always)]
    fn string_run_length(units: &[Self]) -> Option<usize> {
        units.iter().position(|unit| unit.ends_string_run())
    }
    /// The units of `units` at its start that are JSON whitespace.
    #[inline(always)]
    fn whitespace_run_length(units: &[Self]) -> usize {
        units.iter().take_while(|unit| byte_class(unit.unit()) == JsonByteClass::Whitespace).count()
    }
    /// Whether `units` start with the ASCII `text`.
    #[inline(always)]
    fn starts_with_ascii(units: &[Self], text: &[u8]) -> bool {
        units.len() >= text.len() && units.iter().zip(text).all(|(unit, &byte)| unit.unit() == u16::from(byte))
    }
    /// Whether `units` start with `key`, an ASCII key whose text needs no escape, and then
    /// the quote that closes it: the source of a string whose units are `key`.
    #[inline(always)]
    fn starts_key(units: &[Self], key: &[u8]) -> bool {
        units.get(key.len()).is_some_and(|unit| unit.unit() == JSON_QUOTE) && Self::starts_with_ascii(units, key)
    }
    /// `units` as ASCII bytes borrowed in place, when the form already holds one byte per unit;
    /// `None` where a unit is wider than a byte and the caller narrows it.
    #[inline(always)]
    fn ascii_bytes(_units: &[Self]) -> Option<&[u8]> {
        None
    }
    /// `units` as a run of string units.
    fn run(units: &[Self]) -> JsonStringUnits<'_>;
    /// Appends the string units of `units`, a run no escape, quote or control unit ends.
    fn extend_units(units: &[Self], output: &mut Vec<u16>);
}

impl JsonUnit for u8 {
    #[inline(always)] fn unit(self) -> u16 {
        u16::from(self)
    }
    #[inline(always)]
    fn ascii_bytes(units: &[Self]) -> Option<&[u8]> {
        Some(units)
    }
    #[inline(always)]
    fn ends_string_run(self) -> bool {
        STRING_RUN_ENDS[usize::from(self)]
    }
    /// Sixteen bytes at a time where SSE2 is the baseline (`sse2_run_blocks`), with the tail
    /// as one padded block (`sse2_run_tail`); elsewhere a word at a time (`word_run_length`).
    #[inline(always)]
    fn string_run_length(units: &[u8]) -> Option<usize> {
        #[cfg(target_arch = "x86_64")]
        {
            match sse2_run_blocks(units) {
                Ok(length) => Some(length),
                Err(scanned) => sse2_run_tail(&units[scanned..]).map(|length| scanned + length),
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            word_run_length(units)
        }
    }
    /// The whitespace run's first `SIMD_BLOCK_BYTES` bytes by one table load each; from a
    /// whole block of whitespace, the rest in SSE2 blocks (`sse2_whitespace_blocks`), then by
    /// one table load per byte again.
    #[inline(always)]
    fn whitespace_run_length(units: &[u8]) -> usize {
        let head = scalar_whitespace_run(&units[..units.len().min(SIMD_BLOCK_BYTES)]);
        if head < SIMD_BLOCK_BYTES {
            return head;
        }
        #[cfg(target_arch = "x86_64")]
        let blocks = sse2_whitespace_blocks(&units[SIMD_BLOCK_BYTES..]);
        #[cfg(not(target_arch = "x86_64"))]
        let blocks = 0;
        let offset = SIMD_BLOCK_BYTES + blocks;
        offset + scalar_whitespace_run(&units[offset..])
    }
    #[inline(always)]
    fn starts_with_ascii(units: &[u8], text: &[u8]) -> bool {
        units.get(..text.len()) == Some(text)
    }
    #[inline(always)]
    fn starts_key(units: &[u8], key: &[u8]) -> bool {
        units.get(key.len()) == Some(&b'"') && same_ascii(&units[..key.len()], key)
    }
    #[inline(always)]
    fn run(units: &[u8]) -> JsonStringUnits<'_> {
        JsonStringUnits::Run(Units::Ascii(units))
    }
    fn extend_units(units: &[u8], output: &mut Vec<u16>) {
        output.extend(units.iter().map(|&unit| u16::from(unit)));
    }
}

impl JsonUnit for u16 {
    #[inline(always)] fn unit(self) -> u16 {
        self
    }
    /// Eight units at a time where SSE2 is the baseline (`sse2_wide_run_blocks`), with the
    /// tail as one padded block (`sse2_wide_run_tail`); elsewhere one unit at a time.
    #[inline(always)]
    fn string_run_length(units: &[u16]) -> Option<usize> {
        #[cfg(target_arch = "x86_64")]
        {
            match sse2_wide_run_blocks(units) {
                Ok(length) => Some(length),
                Err(scanned) => {
                    sse2_wide_run_tail(&units[scanned..]).map(|length| scanned + length)
                }
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            units.iter().position(|unit| unit.ends_string_run())
        }
    }
    #[inline(always)]
    fn run(units: &[u16]) -> JsonStringUnits<'_> {
        JsonStringUnits::Run(Units::Wide(units))
    }
    fn extend_units(units: &[u16], output: &mut Vec<u16>) {
        output.extend_from_slice(units);
    }
}

/// A byte of UTF-8 text, the host text of decoded text whose units are not made
/// (`JsString::unflattened_host`), which a parse reads without making them. A byte past
/// ASCII is part of a character only a string may hold, and no ASCII terminator falls
/// inside one, so each run a string's terminators bound is whole UTF-8 text.
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Utf8Byte(u8);

impl Utf8Byte {
    /// The bytes of `text` as UTF-8 units.
    fn of(text: &str) -> &[Self] {
        // SAFETY: `Utf8Byte` is a transparent `u8`.
        unsafe { std::slice::from_raw_parts(text.as_ptr().cast::<Self>(), text.len()) }
    }

    /// The bytes of `units`, which are a `str`'s.
    #[inline(always)]
    fn bytes(units: &[Self]) -> &[u8] {
        // SAFETY: `Utf8Byte` is a transparent `u8`.
        unsafe { std::slice::from_raw_parts(units.as_ptr().cast::<u8>(), units.len()) }
    }

    /// The text of a run of whole UTF-8 characters.
    fn text(units: &[Self]) -> &str {
        let bytes = Self::bytes(units);
        debug_assert!(std::str::from_utf8(bytes).is_ok(), "a run bounded by ASCII terminators is whole UTF-8 text");
        // SAFETY: the units are a `str`'s (`Utf8Byte::of`), and a run bounded by ASCII
        // terminators, which no UTF-8 sequence holds, is whole characters of it.
        unsafe { std::str::from_utf8_unchecked(bytes) }
    }
}

impl JsonUnit for Utf8Byte {
    #[inline(always)] fn unit(self) -> u16 {
        u16::from(self.0)
    }
    #[inline(always)]
    fn ends_string_run(self) -> bool {
        STRING_RUN_ENDS[usize::from(self.0)]
    }
    #[inline(always)]
    fn string_run_length(units: &[Self]) -> Option<usize> {
        u8::string_run_length(Self::bytes(units))
    }
    #[inline(always)]
    fn whitespace_run_length(units: &[Self]) -> usize {
        u8::whitespace_run_length(Self::bytes(units))
    }
    #[inline(always)]
    fn starts_with_ascii(units: &[Self], text: &[u8]) -> bool {
        u8::starts_with_ascii(Self::bytes(units), text)
    }
    #[inline(always)]
    fn starts_key(units: &[Self], key: &[u8]) -> bool {
        u8::starts_key(Self::bytes(units), key)
    }
    fn run(units: &[Self]) -> JsonStringUnits<'_> {
        let text = Self::text(units);
        if text.is_ascii() {
            JsonStringUnits::Run(Units::Ascii(text.as_bytes()))
        } else {
            JsonStringUnits::Made(text.encode_utf16().collect())
        }
    }
    fn extend_units(units: &[Self], output: &mut Vec<u16>) {
        output.extend(Self::text(units).encode_utf16());
    }
}

/// A byte of Latin-1 text, whose value is its UTF-16 unit (`Units::Latin1`), so a Latin-1
/// source is parsed in place rather than widened. Its bytes past ASCII are units, not
/// UTF-8, so the scans that `Utf8Byte` shares with `u8` apply to them unchanged: a
/// terminator or whitespace unit is the same byte value in both.
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Latin1(u8);

impl Latin1 {
    /// The units of `bytes`, which are a Latin-1 source's.
    fn of(bytes: &[u8]) -> &[Self] {
        // SAFETY: `Latin1` is a transparent `u8`.
        unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<Self>(), bytes.len()) }
    }

    /// The bytes of `units`.
    #[inline(always)]
    fn bytes(units: &[Self]) -> &[u8] {
        // SAFETY: `Latin1` is a transparent `u8`.
        unsafe { std::slice::from_raw_parts(units.as_ptr().cast::<u8>(), units.len()) }
    }
}

impl JsonUnit for Latin1 {
    #[inline(always)] fn unit(self) -> u16 {
        u16::from(self.0)
    }
    #[inline(always)]
    fn ends_string_run(self) -> bool {
        STRING_RUN_ENDS[usize::from(self.0)]
    }
    #[inline(always)]
    fn string_run_length(units: &[Self]) -> Option<usize> {
        u8::string_run_length(Self::bytes(units))
    }
    #[inline(always)]
    fn whitespace_run_length(units: &[Self]) -> usize {
        u8::whitespace_run_length(Self::bytes(units))
    }
    #[inline(always)]
    fn starts_with_ascii(units: &[Self], text: &[u8]) -> bool {
        u8::starts_with_ascii(Self::bytes(units), text)
    }
    #[inline(always)]
    fn starts_key(units: &[Self], key: &[u8]) -> bool {
        u8::starts_key(Self::bytes(units), key)
    }
    /// The bytes themselves when they are ASCII, which a number literal reads as digits;
    /// `None` past ASCII, where the caller narrows each unit.
    #[inline(always)]
    fn ascii_bytes(units: &[Self]) -> Option<&[u8]> {
        let bytes = Self::bytes(units);
        bytes.is_ascii().then_some(bytes)
    }
    #[inline(always)]
    fn run(units: &[Self]) -> JsonStringUnits<'_> {
        JsonStringUnits::Run(Units::Latin1(Self::bytes(units)))
    }
    fn extend_units(units: &[Self], output: &mut Vec<u16>) {
        output.extend(units.iter().map(|unit| unit.unit()));
    }
}

/// A JSON string's units: a run of the text when it has no escapes and needs no decoding,
/// else the units its escapes or its UTF-8 make.
enum JsonStringUnits<'a> {
    Run(Units<'a>),
    Made(Vec<u16>),
}

impl JsonStringUnits<'_> {
    fn units(&self) -> Units<'_> {
        match self {
            Self::Run(units) => *units,
            Self::Made(units) => Units::Wide(units),
        }
    }
}

/// Parses `text` with `builder`, in the form the string holds: an ASCII or Latin-1 string's
/// bytes need no widening, and decoded text whose units are not made parses from its UTF-8,
/// whose error positions are then counted in UTF-16 units.
fn parse_json_text<B: JsonBuilder>(text: &JsString, builder: &mut B) -> Result<B::Value, JsonParseError> {
    if let Some(host) = text.unflattened_host() {
        return JsonParser::new(Utf8Byte::of(host), builder).parse().map_err(|error| JsonParseError {
            position: String::from_utf8_lossy(&host.as_bytes()[..error.position]).encode_utf16().count(),
            ..error
        });
    }
    match text.units() {
        Units::Ascii(bytes) => JsonParser::new(bytes, builder).parse(),
        Units::Latin1(bytes) => JsonParser::new(Latin1::of(bytes), builder).parse(),
        Units::Wide(units) => JsonParser::new(units, builder).parse(),
    }
}

/// What opening a value leads to: a container opened on the frames, or a complete value.
enum JsonOpened<V> {
    Container,
    Value(V),
}

/// What completing a value leads to: the root's value, or the next member of a container.
enum JsonCompleted<V> {
    Root(V),
    Member,
}

/// What a parse makes of each JSON value: heap values, and, when a reviver walks the parse, the
/// snapshot of it (`JsonRevivalBuilder`).
trait JsonBuilder {
    type Value;
    type Key;
    /// What an object's keys so far determine of the object, which each key advances.
    type Layout;
    /// Whether primitives keep their source text (`primitive`).
    const KEEPS_SOURCE: bool;
    fn literal(&mut self, value: JsonLiteral) -> Self::Value;
    fn number(&mut self, value: f64) -> Self::Value;
    fn integer(&mut self, value: i32) -> Self::Value;
    fn string(&mut self, units: JsonStringUnits<'_>) -> Self::Value;
    /// The layout of an object before its first key, within the object of `parent` when
    /// it is a member's value, directly or within arrays.
    fn layout(&mut self, parent: Option<&Self::Layout>) -> Self::Layout;
    /// The key `layout` expects next, when `matches` its text, an ASCII key that needs no
    /// escape, with its text's length; `layout` then takes the key.
    fn expected_key(&mut self, layout: &mut Self::Layout, matches: impl Fn(&[u8]) -> bool) -> Option<(Self::Key, usize)>;
    /// The key of `units`, which extends `layout`.
    fn key(&mut self, layout: &mut Self::Layout, units: Units<'_>) -> Self::Key;
    fn array(&mut self, values: std::vec::Drain<'_, Self::Value>) -> Self::Value;
    /// An object of `layout`, of `keys` and their `values`, in source order.
    fn object(
        &mut self,
        layout: Self::Layout,
        keys: std::vec::Drain<'_, Self::Key>,
        values: std::vec::Drain<'_, Self::Value>,
    ) -> Self::Value;
    /// A primitive whose token spans `source` of the text parsed, which a reviver's context
    /// exposes; called only when `KEEPS_SOURCE`.
    fn primitive(&mut self, value: Self::Value, source: Range<usize>) -> Self::Value;
    /// The empty stacks a parse gathers members on: a builder that parses again lends its own.
    fn stacks(&mut self) -> JsonStacks<Self::Value, Self::Key, Self::Layout> {
        JsonStacks::with_room()
    }
    /// The stacks a parse finished with, which a builder that parses again keeps.
    fn keep_stacks(&mut self, _stacks: JsonStacks<Self::Value, Self::Key, Self::Layout>) {}
}

#[derive(Clone, Copy)]
enum JsonLiteral {
    Null,
    True,
    False,
}

/// The key an object literal takes as its prototype rather than as a property
/// (`__proto__: value`), which JSON.parse takes as a property.
const PROTO_KEY: &str = "__proto__";

/// Checks the JSON text of a data script's literal (`compile::data_literal`), keeping no
/// values: the text is an array or object literal with the value JSON.parse gives it unless
/// a key is `PROTO_KEY`. It also checks a `JSON.rawJSON` text's syntax.
struct JsonLiteralCheck {
    proto_key: bool,
}

impl JsonBuilder for JsonLiteralCheck {
    type Value = ();
    type Key = ();
    type Layout = ();
    const KEEPS_SOURCE: bool = false;
    fn literal(&mut self, _value: JsonLiteral) {}
    fn number(&mut self, _value: f64) {}
    fn integer(&mut self, _value: i32) {}
    fn string(&mut self, _units: JsonStringUnits<'_>) {}
    fn layout(&mut self, _parent: Option<&()>) {}
    fn expected_key(&mut self, _layout: &mut (), _matches: impl Fn(&[u8]) -> bool) -> Option<((), usize)> {
        None
    }
    fn key(&mut self, _layout: &mut (), units: Units<'_>) {
        self.proto_key |= units.len() == PROTO_KEY.len() && units.iter().eq(PROTO_KEY.encode_utf16());
    }
    fn array(&mut self, _values: std::vec::Drain<'_, ()>) {}
    fn object(&mut self, _layout: (), _keys: std::vec::Drain<'_, ()>, _values: std::vec::Drain<'_, ()>) {}
    fn primitive(&mut self, _value: (), _source: Range<usize>) {}
}

/// The bytes of the JSON array or object text `text` starts with, when the array or object
/// literal of that text evaluates to the value JSON.parse gives it: no key is `PROTO_KEY`.
pub fn json_literal_length(text: &str) -> Option<usize> {
    if !text.starts_with(['[', '{']) {
        return None;
    }
    let mut check = JsonLiteralCheck { proto_key: false };
    let (_, length) = JsonParser::new(Utf8Byte::of(text), &mut check).parse_prefix().ok()?;
    (!check.proto_key).then_some(length)
}

/// A parse of `units` into `builder`'s values, iterative as V8's `JsonParser` is, so nesting
/// is bounded by memory rather than the native stack. Containers gather their members on
/// two stacks the whole parse shares, so each takes exactly its members' storage once. The
/// builder lends the stacks (`JsonBuilder::stacks`) and takes them back when the parse ends.
struct JsonParser<'a, 'b, T, B: JsonBuilder> {
    units: &'a [T],
    index: usize,
    builder: &'b mut B,
    values: Vec<B::Value>,
    keys: Vec<B::Key>,
    frames: Vec<JsonFrame<B::Layout>>,
}

/// The stacks go back to the builder whether the parse finished or stopped at an error.
impl<T, B: JsonBuilder> Drop for JsonParser<'_, '_, T, B> {
    fn drop(&mut self) {
        let stacks = JsonStacks {
            values: std::mem::take(&mut self.values),
            keys: std::mem::take(&mut self.keys),
            frames: std::mem::take(&mut self.frames),
        };
        self.builder.keep_stacks(stacks);
    }
}

impl<'a, 'b, T: JsonUnit, B: JsonBuilder> JsonParser<'a, 'b, T, B> {
    fn new(units: &'a [T], builder: &'b mut B) -> Self {
        let JsonStacks { values, keys, frames } = builder.stacks();
        Self { units, index: 0, builder, values, keys, frames }
    }

    /// The JSON text that is all of the units.
    fn parse(mut self) -> Result<B::Value, JsonParseError> {
        let value = self.root_value()?;
        match self.token() {
            None => Ok(value),
            Some(_) => Err(self.error(JsonSyntax::NonWhitespaceAfter)),
        }
    }

    /// The JSON text the units start with, and the units it spans.
    fn parse_prefix(mut self) -> Result<(B::Value, usize), JsonParseError> {
        let value = self.root_value()?;
        Ok((value, self.index))
    }

    /// The JSON value at the current unit, which the parse moves past. The frames leave the
    /// parser meanwhile, so a frame's layout and the parser are borrowed apart.
    fn root_value(&mut self) -> Result<B::Value, JsonParseError> {
        let mut frames = std::mem::take(&mut self.frames);
        let value = self.value(&mut frames);
        self.frames = frames;
        value
    }

    /// `root_value`, gathering containers on `frames`: each value opened is completed, and the
    /// completed value that is the root's ends the parse.
    fn value(&mut self, frames: &mut Vec<JsonFrame<B::Layout>>) -> Result<B::Value, JsonParseError> {
        loop {
            let value = match self.open(frames)? {
                JsonOpened::Container => continue,
                JsonOpened::Value(value) => value,
            };
            if let JsonCompleted::Root(value) = self.complete(frames, value)? {
                return Ok(value);
            }
        }
    }

    /// Starts the value at the current unit, which the parse moves past: a primitive or an
    /// empty container is complete, and a container with members is opened on `frames`.
    fn open(&mut self, frames: &mut Vec<JsonFrame<B::Layout>>) -> Result<JsonOpened<B::Value>, JsonParseError> {
        let class = self.token().map(byte_class);
        let start = self.index;
        let value = match class {
            Some(JsonByteClass::ObjectStart) => return self.open_object(frames),
            Some(JsonByteClass::ArrayStart) => return self.open_array(frames),
            Some(JsonByteClass::Quote) => {
                let text = self.string()?;
                self.builder.string(text)
            }
            Some(JsonByteClass::Minus | JsonByteClass::Digit) => match self.number()? {
                JsonNumber::Integer(value) => self.builder.integer(value),
                JsonNumber::Float(value) => self.builder.number(value),
            },
            Some(JsonByteClass::Null) => self.literal(b"null", JsonLiteral::Null)?,
            Some(JsonByteClass::True) => self.literal(b"true", JsonLiteral::True)?,
            Some(JsonByteClass::False) => self.literal(b"false", JsonLiteral::False)?,
            _ => return Err(self.error(JsonSyntax::Unexpected)),
        };
        Ok(JsonOpened::Value(self.with_source(value, start)))
    }

    /// `value` of the primitive that starts at unit `start`, with its source range when the
    /// builder keeps source text.
    #[inline(always)]
    fn with_source(&mut self, value: B::Value, start: usize) -> B::Value {
        if B::KEEPS_SOURCE {
            self.builder.primitive(value, start..self.index)
        } else {
            value
        }
    }

    /// Opens the object at the current unit: an empty object is complete, and an object with
    /// members is opened on `frames`, its first key read.
    fn open_object(&mut self, frames: &mut Vec<JsonFrame<B::Layout>>) -> Result<JsonOpened<B::Value>, JsonParseError> {
        self.index += 1;
        let parent = frames.iter().rev().find_map(|frame| match frame {
            JsonFrame::Object { layout, .. } => Some(layout),
            JsonFrame::Array { .. } => None,
        });
        let layout = self.builder.layout(parent);
        let (values, keys) = (self.values.len(), self.keys.len());
        // One read of the next token decides both the empty object and the first key, which
        // `quoted_key` takes from the quote already read.
        match self.token() {
            Some(JSON_OBJECT_END) => {
                self.index += 1;
                let value = self.builder.object(layout, self.keys.drain(keys..), self.values.drain(values..));
                return Ok(JsonOpened::Value(value));
            }
            Some(JSON_QUOTE) => {}
            _ => return Err(self.error(JsonSyntax::ExpectedPropertyNameOrBrace)),
        }
        // The frame holds the layout its keys advance in place: a copy made right after
        // their stores would read them back through memory.
        frames.push(JsonFrame::Object { values, keys, layout });
        let Some(JsonFrame::Object { layout, .. }) = frames.last_mut() else {
            unreachable!("the frame is an object's")
        };
        self.quoted_key(layout)?;
        Ok(JsonOpened::Container)
    }

    /// Opens the array at the current unit: an empty array is complete, and an array with
    /// members is opened on `frames`.
    fn open_array(&mut self, frames: &mut Vec<JsonFrame<B::Layout>>) -> Result<JsonOpened<B::Value>, JsonParseError> {
        self.index += 1;
        let values = self.values.len();
        if self.token() == Some(JSON_ARRAY_END) {
            self.index += 1;
            let value = self.builder.array(self.values.drain(values..));
            return Ok(JsonOpened::Value(value));
        }
        frames.push(JsonFrame::Array { values });
        Ok(JsonOpened::Container)
    }

    /// Hands the completed `value` to the innermost container on `frames`, which takes it as
    /// its next member. A container the value closes is complete in turn, so the value moves
    /// up the frames until a comma leaves a container for its next member, or no frame is left.
    fn complete(
        &mut self,
        frames: &mut Vec<JsonFrame<B::Layout>>,
        mut value: B::Value,
    ) -> Result<JsonCompleted<B::Value>, JsonParseError> {
        loop {
            let Some(frame) = frames.last_mut() else {
                return Ok(JsonCompleted::Root(value));
            };
            let token = self.token();
            self.values.push(value);
            value = match (frame, token) {
                (JsonFrame::Array { .. }, Some(JSON_COMMA)) => {
                    self.index += 1;
                    return Ok(JsonCompleted::Member);
                }
                (JsonFrame::Array { values }, Some(JSON_ARRAY_END)) => {
                    self.index += 1;
                    let values = *values;
                    frames.pop();
                    self.builder.array(self.values.drain(values..))
                }
                (JsonFrame::Array { .. }, _) => return Err(self.error(JsonSyntax::ExpectedCommaOrBracket)),
                (JsonFrame::Object { layout, .. }, Some(JSON_COMMA)) => {
                    self.index += 1;
                    self.member_key(layout, JsonSyntax::ExpectedDoubleQuotedPropertyName)?;
                    return Ok(JsonCompleted::Member);
                }
                (JsonFrame::Object { .. }, Some(JSON_OBJECT_END)) => self.close_object(frames),
                (JsonFrame::Object { .. }, _) => return Err(self.error(JsonSyntax::ExpectedCommaOrBrace)),
            };
        }
    }

    /// Closes the object on top of `frames` at its `}`, which the parse moves past: the
    /// object of its gathered keys and values.
    fn close_object(&mut self, frames: &mut Vec<JsonFrame<B::Layout>>) -> B::Value {
        self.index += 1;
        let Some(JsonFrame::Object { values, keys, layout }) = frames.pop() else {
            unreachable!("the frame is an object's")
        };
        self.builder.object(layout, self.keys.drain(keys..), self.values.drain(values..))
    }

    /// `syntax` at the current unit.
    #[cold]
    fn error(&self, syntax: JsonSyntax) -> JsonParseError {
        JsonParseError {
            syntax,
            position: self.index,
        }
    }

    /// A member's key and its colon, which extend `layout`; `missing` when no key starts here.
    #[inline(always)]
    fn member_key(&mut self, layout: &mut B::Layout, missing: JsonSyntax) -> Result<(), JsonParseError> {
        if self.token() != Some(JSON_QUOTE) {
            return Err(self.error(missing));
        }
        self.quoted_key(layout)
    }

    /// `member_key` of a key whose opening quote is the current unit, which the caller read.
    #[inline(always)]
    fn quoted_key(&mut self, layout: &mut B::Layout) -> Result<(), JsonParseError> {
        let text = &self.units[self.index + 1..];
        let key = match self.builder.expected_key(layout, |key| T::starts_key(text, key)) {
            Some((key, length)) => {
                self.index += length + 2;
                key
            }
            None => {
                let text = self.string()?;
                self.builder.key(layout, text.units())
            }
        };
        self.keys.push(key);
        if self.token() != Some(JSON_COLON) {
            return Err(self.error(JsonSyntax::ExpectedColon));
        }
        self.index += 1;
        Ok(())
    }

    /// A literal, whose first unit the caller matched.
    fn literal(&mut self, expected: &[u8], value: JsonLiteral) -> Result<B::Value, JsonParseError> {
        if !T::starts_with_ascii(&self.units[self.index..], expected) {
            return Err(self.literal_mismatch(expected));
        }
        self.index += expected.len();
        Ok(self.builder.literal(value))
    }

    /// The error of `literal` at the first unit that differs from `expected`, or at the end of
    /// the text when it ends first.
    #[cold]
    fn literal_mismatch(&self, expected: &[u8]) -> JsonParseError {
        let matched = self.units[self.index..]
            .iter()
            .zip(expected)
            .take_while(|(unit, byte)| unit.unit() == u16::from(**byte))
            .count();
        JsonParseError {
            syntax: JsonSyntax::Unexpected,
            position: self.index + matched,
        }
    }

    /// A string's code units, from its opening quote, which the caller matched: a run of the
    /// source when it has no escapes. Inline, so the run reaches the builder in registers.
    #[inline(always)]
    fn string(&mut self) -> Result<JsonStringUnits<'a>, JsonParseError> {
        self.index += 1;
        let units = self.units;
        let start = self.index;
        let run = T::string_run_length(&units[start..]);
        if let Some(length) = run
            && units[start + length].unit() == JSON_QUOTE
        {
            self.index = start + length + 1;
            return Ok(T::run(&units[start..start + length]));
        }
        self.escaped_string()
    }

    /// `string` of a string with escapes, or of none, from its first unit.
    #[cold]
    fn escaped_string(&mut self) -> Result<JsonStringUnits<'a>, JsonParseError> {
        let units = self.units;
        let mut output = Vec::new();
        loop {
            let run = &units[self.index..];
            let length = T::string_run_length(run).unwrap_or(run.len());
            T::extend_units(&run[..length], &mut output);
            self.index += length;
            match self.peek() {
                Some(JSON_QUOTE) => {
                    self.index += 1;
                    return Ok(JsonStringUnits::Made(output));
                }
                Some(JSON_ESCAPE) => {
                    self.index += 1;
                    self.escape(&mut output)?;
                }
                Some(_) => return Err(self.error(JsonSyntax::BadControlCharacter)),
                None => return Err(self.error(JsonSyntax::UnterminatedString)),
            }
        }
    }

    /// The escape after a backslash.
    fn escape(&mut self, output: &mut Vec<u16>) -> Result<(), JsonParseError> {
        let escaped = match self.peek() {
            None => return Err(self.error(JsonSyntax::Unexpected)),
            Some(unit) if unit == JSON_QUOTE || unit == JSON_ESCAPE || unit == b'/' as u16 => unit,
            Some(unit) if unit == b'b' as u16 => JSON_BACKSPACE,
            Some(unit) if unit == b'f' as u16 => JSON_FORM_FEED,
            Some(unit) if unit == b'n' as u16 => LINE_FEED,
            Some(unit) if unit == b'r' as u16 => CARRIAGE_RETURN,
            Some(unit) if unit == b't' as u16 => b'\t' as u16,
            Some(unit) if unit == b'u' as u16 => {
                self.index += 1;
                output.push(self.hex_escape()?);
                return Ok(());
            }
            Some(_) => return Err(self.error(JsonSyntax::BadEscape)),
        };
        self.index += 1;
        output.push(escaped);
        Ok(())
    }

    fn hex_escape(&mut self) -> Result<u16, JsonParseError> {
        let mut value = 0;
        for _ in 0..JSON_HEX_ESCAPE_DIGITS {
            let Some(digit) = self.peek().and_then(hex_digit) else {
                return Err(self.error(JsonSyntax::BadUnicodeEscape));
            };
            self.index += 1;
            value = value * 16 + digit;
        }
        Ok(value)
    }

    /// A number, whose first unit, a digit or a minus sign, the caller matched. JSON's grammar
    /// admits the literal (no leading zeros, digits after a point and after an exponent mark),
    /// and `number_value` reads its value.
    fn number(&mut self) -> Result<JsonNumber, JsonParseError> {
        let start = self.index;
        let negative = self.take_if(b'-');
        let digits = self.index;
        self.integer_part()?;
        let point = self.take_if(b'.');
        if point {
            if self.digit_value().is_none() {
                return Err(self.error(JsonSyntax::UnterminatedFraction));
            }
            self.skip_digits();
        }
        // A literal without an exponent whose digits are few is valued here, in place: no second
        // scan of the literal (`number_value`). `-0` stays a double.
        if !exponent_mark(self.peek())
            && let Some(value) = self.short_number(negative, digits)
        {
            return Ok(value);
        }
        if exponent_mark(self.peek()) {
            self.index += 1;
            self.exponent()?;
        }
        Ok(self.number_value(start))
    }

    /// The value of the unsigned-digit run from `digits` to the current unit, with its sign and
    /// an optional point, when it has at most `JSON_SHORT_NUMBER_DIGITS` digits and no exponent.
    /// Its mantissa is exact in a `u64` and a double; a fraction is one correctly rounded division
    /// by an exact power of ten, the Clinger fast path. `None` otherwise, and for `-0`.
    #[inline(always)]
    fn short_number(&self, negative: bool, digits: usize) -> Option<JsonNumber> {
        let literal = &self.units[digits..self.index];
        let point = literal.iter().position(|unit| unit.unit() == u16::from(b'.'));
        let (integer, fraction) = match point {
            Some(point) => (&literal[..point], &literal[point + 1..]),
            None => (literal, &literal[literal.len()..]),
        };
        if integer.len() + fraction.len() > JSON_SHORT_NUMBER_DIGITS {
            return None;
        }
        let mantissa = literal
            .iter()
            .filter_map(|unit| decimal_digit(unit.unit()))
            .fold(0_u64, |value, digit| value * 10 + digit);
        if point.is_none() {
            if negative && mantissa == 0 {
                return None;
            }
            return Some(match i32::try_from(mantissa) {
                Ok(magnitude) => JsonNumber::Integer(if negative { -magnitude } else { magnitude }),
                Err(_) => {
                    let value = mantissa as f64;
                    JsonNumber::Float(if negative { -value } else { value })
                }
            });
        }
        let power = *JSON_SHORT_POWERS_OF_TEN.get(fraction.len())?;
        let value = mantissa as f64 / power;
        Some(JsonNumber::Float(if negative { -value } else { value }))
    }

    /// The integer part of a number from the current unit, which the parse moves past. A
    /// leading zero ends the part, so a digit after it is an error.
    #[inline(always)]
    fn integer_part(&mut self) -> Result<(), JsonParseError> {
        match self.peek() {
            Some(JSON_DIGIT_START) => {
                self.index += 1;
                if self.digit_value().is_some() {
                    return Err(self.error(JsonSyntax::Unexpected));
                }
            }
            Some(JSON_DIGIT_START..=JSON_DIGIT_END) => self.skip_digits(),
            _ => return Err(self.error(JsonSyntax::NoNumberAfterMinus)),
        }
        Ok(())
    }

    /// The signed exponent's digits after an exponent mark, which the parse moves past.
    fn exponent(&mut self) -> Result<(), JsonParseError> {
        if !self.take_if(b'-') {
            self.take_if(b'+');
        }
        if self.digit_value().is_none() {
            return Err(self.error(JsonSyntax::ExponentMissingNumber));
        }
        self.skip_digits();
        Ok(())
    }

    /// The value of the admitted number literal from unit `start` to the current one, read as
    /// ASCII by `number_parse`: an `i32` when it is an integer that fits one, else a double.
    /// `-0` is a double, since an integer's negative zero is not one.
    fn number_value(&self, start: usize) -> JsonNumber {
        let literal = &self.units[start..self.index];
        let mut inline = [0_u8; JSON_INLINE_NUMBER_LENGTH];
        let long;
        let bytes: &[u8] = if let Some(bytes) = T::ascii_bytes(literal) {
            bytes
        } else if literal.len() <= JSON_INLINE_NUMBER_LENGTH {
            for (byte, unit) in inline.iter_mut().zip(literal) {
                *byte = unit.unit() as u8;
            }
            &inline[..literal.len()]
        } else {
            long = literal.iter().map(|unit| unit.unit() as u8).collect::<Vec<_>>();
            &long
        };
        // The grammar admitted at least one digit, which `scan_decimal` reads.
        let decimal = scan_decimal(bytes).expect("the grammar admits only decimal literals");
        if decimal.unsigned.len() == decimal.integer.len()
            && let Some(magnitude) = exact_integer(decimal.integer, DECIMAL_RADIX)
            && let Ok(magnitude) = i32::try_from(magnitude)
            && !(decimal.negative && magnitude == 0)
        {
            return JsonNumber::Integer(if decimal.negative { -magnitude } else { magnitude });
        }
        JsonNumber::Float(decimal_value(&decimal))
    }

    /// The decimal digits from the current unit, which the parse moves past.
    #[inline(always)]
    fn skip_digits(&mut self) {
        let units = self.units;
        let mut at = self.index;
        while units.get(at).is_some_and(|unit| decimal_digit(unit.unit()).is_some()) {
            at += 1;
        }
        self.index = at;
    }

    /// The value of the current unit when it is a decimal digit.
    #[inline(always)]
    fn digit_value(&self) -> Option<u64> {
        self.peek().and_then(decimal_digit)
    }

    /// The next unit past whitespace, which the parse moves past.
    #[inline(always)]
    fn token(&mut self) -> Option<u16> {
        match self.peek() {
            Some(unit) if byte_class(unit) == JsonByteClass::Whitespace => self.token_after_whitespace(),
            unit => unit,
        }
    }

    /// The next unit after the run of whitespace at the current unit, which the parse moves
    /// past. Whitespace is one or two bytes in most JSON, so the run is scanned in place.
    fn token_after_whitespace(&mut self) -> Option<u16> {
        self.index += T::whitespace_run_length(&self.units[self.index..]);
        self.peek()
    }

    #[inline(always)]
    fn take_if(&mut self, expected: u8) -> bool {
        if self.peek() == Some(u16::from(expected)) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    #[inline(always)]
    fn peek(&self) -> Option<u16> {
        self.units.get(self.index).map(|unit| unit.unit())
    }
}

/// The value of `unit` when it is a decimal digit.
#[inline(always)]
fn decimal_digit(unit: u16) -> Option<u64> {
    let digit = unit.wrapping_sub(JSON_DIGIT_START);
    (digit <= JSON_DIGIT_END - JSON_DIGIT_START).then_some(u64::from(digit))
}

fn hex_digit(unit: u16) -> Option<u16> {
    match unit {
        value if (b'0' as u16..=b'9' as u16).contains(&value) => Some(value - b'0' as u16),
        value if (b'a' as u16..=b'f' as u16).contains(&value) => {
            Some(value - b'a' as u16 + JSON_HEX_LETTER_VALUE_START)
        }
        value if (b'A' as u16..=b'F' as u16).contains(&value) => {
            Some(value - b'A' as u16 + JSON_HEX_LETTER_VALUE_START)
        }
        _ => None,
    }
}

impl Vm {
    fn json_object(&mut self) -> Value {
        let prototype = self.realm_object_prototype(self.realm.globals);
        self.heap.alloc_object(Self::empty_object(prototype))
    }

    pub(super) fn json_raw_json(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let text = self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let wide = text.units().wide();
        let units: &[u16] = &wide;
        // One primitive that is the whole text, with no whitespace around it, reported as
        // V8's `JsonParser::ParseRawJson` reports anything else.
        let Some(&first) = units.first() else {
            return self.syntax_error_result(p, JSON_INVALID_RAW_TEXT);
        };
        let primitive = matches!(
            first,
            JSON_QUOTE | JSON_MINUS | JSON_DIGIT_START..=JSON_DIGIT_END | JSON_NULL_INITIAL | JSON_TRUE_INITIAL | JSON_FALSE_INITIAL
        );
        let checked = if primitive {
            JsonParser::new(units, &mut JsonLiteralCheck { proto_key: false }).parse_prefix().map(|(_, length)| length)
        } else {
            Err(JsonParseError {
                syntax: JsonSyntax::Unexpected,
                position: 0,
            })
        };
        match checked {
            Ok(length) if length == units.len() => {}
            Ok(_) => return self.syntax_error_result(p, JSON_INVALID_RAW_TEXT),
            Err(error) => {
                return self.syntax_error_result(p, &json_syntax_message(units, error.syntax, error.position));
            }
        }
        let object = self.json_object();
        self.with_call_roots([object], |vm| {
            if let Some(data) = vm.object_data_mut(object) {
                data.set_proto(Value::NULL);
                data.set_raw_json();
            }
            let raw_atom = vm.intern_atom("rawJSON");
            let raw_text_value = vm.heap.alloc_string(text);
            vm.with_call_roots([raw_text_value], |vm| {
                let key = vm.heap.alloc_string(vm.atom_name(raw_atom).into());
                let descriptor = PropertyDescriptorRecord {
                    writable: Some(false),
                    configurable: Some(false),
                    ..PropertyDescriptorRecord::data(raw_text_value)
                };
                vm.define_own_property_record(p, object, key, descriptor)?;
                vm.object_prevent_extensions(p, &[object])
            })
            .map(|_| object)
        })
    }

    pub(super) fn json_is_raw_json(&self, args: &[Value]) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let is_raw = self.object_data(value).is_some_and(Object::is_raw_json);
        Ok(if is_raw { Value::TRUE } else { Value::FALSE })
    }

    pub(super) fn json_parse(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let text = self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let reviver = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(reviver) {
            // Nothing revisits the parse, so values go straight to the heap.
            let mut builder = JsonHeapBuilder::new(self);
            let parsed = parse_json_text(&text, &mut builder);
            let built = builder.finish(parsed);
            return match built {
                Ok(value) => Ok(value),
                Err(JsonBuildError::Parse(error)) => self.json_syntax_error(p, &text, error),
                Err(JsonBuildError::Runtime(error)) => Err(error),
            };
        }
        // One parse builds the heap values and the snapshot the reviver walks.
        let mut builder = JsonRevivalBuilder::new(self);
        let parsed = parse_json_text(&text, &mut builder);
        let (value, mut snapshot) = match builder.finish(parsed, text.clone()) {
            Ok(built) => built,
            Err(JsonBuildError::Parse(error)) => return self.json_syntax_error(p, &text, error),
            Err(JsonBuildError::Runtime(error)) => return Err(error),
        };
        let reviver_root = self.heap.root(reviver);
        let holder = self.json_object();
        let empty = self.intern_atom("");
        let result = self.with_call_roots([holder], |vm| {
            vm.set_property(holder, empty, value)?;
            vm.json_internalize(p, holder, empty, Some(snapshot.root), &snapshot, reviver_root)
        });
        self.heap.release_root(reviver_root);
        for root in snapshot.roots.drain(..) {
            self.heap.release_root(root);
        }
        result
    }

    /// The value of the JSON text that `text` starts with, which compilation checked
    /// (`json_literal_length`): what the array or object literal of that text evaluates to.
    pub(super) fn json_literal(&mut self, text: &str) -> Result<Value, JsError> {
        let mut builder = JsonHeapBuilder::new(self);
        let parsed = JsonParser::new(Utf8Byte::of(text), &mut builder).parse_prefix();
        builder
            .finish(parsed.map(|(value, _)| value))
            .map_err(|_| JsError::validation("a data literal is not the JSON text its compilation checked".into()))
    }

    /// The SyntaxError of `error`, at a UTF-16 position of `text`.
    #[cold]
    fn json_syntax_error(&mut self, p: &ResidualProgram, text: &JsString, error: JsonParseError) -> Result<Value, JsError> {
        let units = text.units().wide();
        self.syntax_error_result(p, &json_syntax_message(&units, error.syntax, error.position))
    }

    /// The source text of the primitive at snapshot `node`, when the holder's `current` value is
    /// still SameValue to the primitive's parse-time value.
    fn json_source_text(&self, snapshot: &JsonSnapshot, node: u32, current: Value) -> Option<JsString> {
        let (value, source) = snapshot.primitive(node)?;
        self.same_value(current, value).then(|| snapshot.source_text(source))
    }

    /// InternalizeJSONProperty of `holder[key]`, walking the parse-time `snapshot` from `source`,
    /// the node of the value the parse made there (`None` once the walk leaves the parse). The
    /// key is an atom, as the walk's callers hold it: an index's is `index_key_atom`'s.
    fn json_internalize(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key_atom: Atom,
        source: Option<u32>,
        snapshot: &JsonSnapshot,
        reviver_root: crate::heap::RootId,
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        let holder_root = self.heap.root(holder);
        let mut value_root = None;
        let mut key_root = None;
        let mut context_root = None;
        let result = (|| {
            let holder = self.heap.root_value(holder_root).unwrap();
            let value = self.get_property(p, holder, key_atom)?;
            let value_handle = self.heap.root(value);
            value_root = Some(value_handle);
            let source_text = source.and_then(|node| self.json_source_text(snapshot, node, value));
            if self.is_array(p, value)? {
                let array = self.heap.root_value(value_handle).unwrap();
                let length = self.array_like_length(p, array)?;
                for index in 0..length {
                    let array = self.heap.root_value(value_handle).unwrap();
                    let child_key = self.index_key_atom(index);
                    let child_source = source.and_then(|node| snapshot.array_child(node, index));
                    let child = self.json_internalize(p, array, child_key, child_source, snapshot, reviver_root)?;
                    self.json_update_property(p, value_handle, child_key, child)?;
                }
            } else if self.is_object_like(value) {
                let object = self.heap.root_value(value_handle).unwrap();
                let keys = self.json_enumerable_keys(p, object)?;
                for key_atom in keys {
                    let object = self.heap.root_value(value_handle).unwrap();
                    let child_source = source.and_then(|node| snapshot.object_child(node, key_atom));
                    let child = self.json_internalize(p, object, key_atom, child_source, snapshot, reviver_root)?;
                    self.json_update_property(p, value_handle, key_atom, child)?;
                }
            }
            let key_value = self.atom_string(key_atom);
            let key_handle = self.heap.root(key_value);
            key_root = Some(key_handle);
            let context = self.json_object();
            let context_handle = self.heap.root(context);
            context_root = Some(context_handle);
            if let Some(source) = source_text {
                let source_atom = self.intern_atom("source");
                let source_value = self.heap.alloc_string(source);
                let source_root = self.heap.root(source_value);
                let context = self.heap.root_value(context_handle).unwrap();
                let source_value = self.heap.root_value(source_root).unwrap();
                let set = self.set_property(context, source_atom, source_value);
                self.heap.release_root(source_root);
                set?;
            }
            let reviver = self.heap.root_value(reviver_root).unwrap();
            let holder = self.heap.root_value(holder_root).unwrap();
            let value = self.heap.root_value(value_handle).unwrap();
            self.call_value(
                p,
                reviver,
                holder,
                &[
                    self.heap.root_value(key_handle).unwrap(),
                    value,
                    self.heap.root_value(context_handle).unwrap(),
                ],
            )
        })();
        for root in [Some(holder_root), value_root, key_root, context_root]
            .into_iter()
            .flatten()
        {
            self.heap.release_root(root);
        }
        result
    }

    fn json_update_property(
        &mut self,
        p: &ResidualProgram,
        holder: RootId,
        key: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        self.with_call_roots([value], |vm| {
            let key = vm.atom_string(key);
            vm.with_call_roots([key], |vm| {
                let holder = vm.heap.root_value(holder).unwrap();
                if value.is_undefined() {
                    vm.object_delete_property(p, &[holder, key]).map(|_| ())
                } else {
                    vm.define_own_property_record(p, holder, key, PropertyDescriptorRecord::data(value))
                        .map(|_| ())
                }
            })
        })
    }

    /// JSON.stringify(value, replacer, space): the plain writer (`json_stringify_steady`) first,
    /// which answers plain data with no replacer or gap; otherwise the replacer and the gap,
    /// read in order, then the one writer (`json_run`) under them.
    pub(super) fn json_stringify(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if self.declined_fast.take() != Some(Native::JsonStringify)
            && let Some(text) = self.json_stringify_steady(args)
        {
            return Ok(text);
        }
        let argument = |index: usize| args.get(index).copied().unwrap_or(Value::UNDEFINED);
        let space = argument(JSON_SPACE_ARGUMENT);
        let (replacer, property_list) = self.json_replacer(p, argument(JSON_REPLACER_ARGUMENT))?;
        let gap = self.with_call_roots([space], |vm| vm.json_gap(p, space))?;
        let options = JsonOptions { replacer, property_list, gap };
        self.json_run(Some(p), args.first().copied().unwrap_or(Value::UNDEFINED), options)
            .map_err(|stop| match stop {
                JsonStop::Thrown(error) => error,
                JsonStop::Plain => unreachable!("a run with a program takes every step"),
            })
    }

    fn json_replacer(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<(Option<Value>, Option<Vec<Atom>>), JsError> {
        if self.is_function(value) {
            return Ok((Some(value), None));
        }
        if !self.is_object_like(value) || !self.is_array(p, value)? {
            return Ok((None, None));
        }
        self.with_call_roots([value], |vm| {
            let length = vm.array_like_length(p, value)?;
            let mut names = Vec::new();
            for index in 0..length {
                let item = vm.get_index(p, value, Value::number(index as f64))?;
                let name = vm.with_call_roots([item], |vm| -> Result<Option<Atom>, JsError> {
                    let accepted = item.as_number().is_some()
                        || vm.heap.is_string(item)
                        || vm.json_boxed_string_or_number(item).is_some();
                    if !accepted {
                        return Ok(None);
                    }
                    let name = vm.coerce_js_string(p, item)?;
                    Ok(Some(vm.intern_js_atom(&name)))
                })?;
                if let Some(name) = name
                    && !names.contains(&name)
                {
                    names.push(name);
                }
            }
            Ok((None, Some(names)))
        })
    }

    fn json_boxed_string_or_number(&self, value: Value) -> Option<Value> {
        self.boxed_primitive_data(value)
            .and_then(|(brand, payload)| {
                matches!(
                    brand,
                    crate::BuiltinBrand::StringObject | crate::BuiltinBrand::NumberObject
                )
                .then_some(payload)
            })
    }

    fn json_gap(&mut self, p: &ResidualProgram, value: Value) -> Result<String, JsError> {
        let brand = self.boxed_primitive_data(value).map(|(brand, _)| brand);
        let number_box = brand == Some(crate::BuiltinBrand::NumberObject);
        let string_box = brand == Some(crate::BuiltinBrand::StringObject);
        let value = if number_box {
            Value::number(self.to_number(p, value)?)
        } else if string_box {
            let text = self.coerce_js_string(p, value)?;
            self.heap.alloc_string(text)
        } else {
            value
        };
        if let Some(number) = value.as_number() {
            let count = if number.is_nan() || number <= 0.0 {
                0
            } else {
                number.floor().min(10.0) as usize
            };
            return Ok(" ".repeat(count));
        }
        if let Some(text) = self.heap.string(value) {
            return Ok(String::from_utf16_lossy(
                &text.units().slice(0..text.units().len().min(10)).wide(),
            ));
        }
        Ok(String::new())
    }

    /// SerializeJSONProperty's steps for a member the writer does not write as it is
    /// (`json_write_member`): [[Get]] of `key` unless its value was read already (`value`),
    /// `toJSON`, the replacer, then SerializeJSONValue. Each may run guest code, after which the
    /// writer re-proves its plans (`JsonWriter::after_guest_code`).
    fn json_serialize_property(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: JsonKey,
        value: Option<Value>,
        w: &mut JsonWriter,
    ) -> JsonStep {
        let _stack = self.enter_stack()?;
        let (to_json, replacer) = (w.to_json, w.options.replacer);
        let value = self.with_call_roots([holder], |vm| {
            vm.json_property_value(p, holder, key, value, to_json, replacer)
        })?;
        w.after_guest_code(self.json_epoch());
        self.with_call_roots([value], |vm| vm.json_serialize_value(p, value, w))
    }

    /// SerializeJSONProperty's value: `Get(holder, key)` unless `value` was read already,
    /// replaced by its `toJSON` call's result, then by the replacer's.
    fn json_property_value(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: JsonKey,
        value: Option<Value>,
        to_json: Atom,
        replacer: Option<Value>,
    ) -> Result<Value, JsError> {
        let value = match (value, key) {
            (Some(value), _) => value,
            (None, JsonKey::Index(index)) => self.get_index(p, holder, Value::number(index as f64))?,
            (None, JsonKey::Name(atom)) => self.get_property(p, holder, atom)?,
        };
        let value = self.with_call_roots([value], |vm| vm.json_to_json(p, key, value, to_json))?;
        let Some(replacer) = replacer else {
            return Ok(value);
        };
        self.with_call_roots([value, replacer], |vm| {
            let key_value = vm.json_key_value(key);
            vm.call_value(p, replacer, holder, &[key_value, value])
        })
    }

    /// `value`, or the result of its `toJSON` method called with `key`, for an object or
    /// BigInt whose `toJSON` is callable.
    fn json_to_json(
        &mut self,
        p: &ResidualProgram,
        key: JsonKey,
        value: Value,
        to_json: Atom,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(value) && !matches!(self.heap.get(value), Some(Cell::BigInt(_))) {
            return Ok(value);
        }
        let method = self.get_property(p, value, to_json)?;
        if !self.is_function(method) {
            return Ok(value);
        }
        self.with_call_roots([method], |vm| {
            let key_value = vm.json_key_value(key);
            vm.call_value(p, method, value, &[key_value])
        })
    }

    /// Whether `Get(value, "toJSON")` finds nothing, decided without running guest code, for
    /// an object the serializer reads as it is, along a chain of arrays (such as
    /// `Array.prototype`) whose own shapes lack the key and of ordinary objects
    /// (`plain_property_lookup`). A boxed primitive, whose serialization coerces it, and any
    /// other object answer `false`. A caller that has not decided the boxed fact of `value`
    /// reads it here; one that has (`json_plain_plan`) calls `json_chain_lacks_to_json`.
    fn json_lacks_to_json(&self, value: Value, to_json: Atom) -> bool {
        self.boxed_primitive_data(value).is_none() && self.json_chain_lacks_to_json(value, to_json)
    }

    /// `json_lacks_to_json` for an object already known not to be a boxed primitive: the chain
    /// walk alone.
    fn json_chain_lacks_to_json(&self, value: Value, to_json: Atom) -> bool {
        let key = PropertyKey::string(to_json);
        let mut current = value;
        loop {
            let Some(array) = self.heap.array(current) else {
                return self.plain_property_lookup(current, key) == Some(None);
            };
            if array.is_arguments_object()
                || array.has_side_descriptors()
                || self.property_shape_slot(array.shape(), key).is_some()
            {
                return false;
            }
            current = array.proto();
            if current.is_null() {
                return true;
            }
        }
    }

    /// A key's string value, allocated last before the call that receives it. An index's is
    /// its atom's string, as ToPropertyKey's index keys are (`to_property_key`).
    fn json_key_value(&mut self, key: JsonKey) -> Value {
        match key {
            // A pre-built index atom keeps its string; a larger index stays a transient
            // string, so stringifying a long array does not grow the atom table.
            JsonKey::Index(index) => match u32::try_from(index).ok().and_then(|small| self.array_index_atoms.get(small)) {
                Some(atom) => self.atom_string(atom),
                None => {
                    let mut buffer = itoa::Buffer::new();
                    self.heap.alloc_string(JsString::from_str(buffer.format(index)))
                }
            },
            JsonKey::Name(atom) => {
                let text = self.atom_value(atom);
                self.heap.alloc_string(text)
            }
        }
    }

    /// SerializeJSONValue of `value`, after `toJSON` and the replacer: plain data as it is
    /// (`json_write_primitive`, `json_write_plain_container`), else the specification's steps.
    /// Unwrapping a boxed primitive may run guest code.
    fn json_serialize_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        w: &mut JsonWriter,
    ) -> JsonStep {
        if let Some(written) = self.json_write_primitive(value, &mut w.output) {
            return Ok(written);
        }
        if let Some(written) = self.json_write_plain_container(Some(p), value, w)? {
            return Ok(written);
        }
        if self.is_function(value) {
            return Ok(false);
        }
        let unboxed = self.json_unbox(p, value)?;
        if unboxed != value {
            w.after_guest_code(self.json_epoch());
            return self.json_serialize_value(p, unboxed, w);
        }
        if let Some(raw) = self.json_raw_text(value) {
            w.output.extend_from_slice(raw.as_bytes());
            return Ok(true);
        }
        self.json_serialize_heap_value(p, value, w)
    }

    /// SerializeJSONValue's last steps, for a value no earlier step writes: an object or
    /// array by the specification's steps (an exotic object is an array if IsArray says so),
    /// a BigInt's TypeError, or an omitted internal value.
    fn json_serialize_heap_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        w: &mut JsonWriter,
    ) -> JsonStep {
        let is_array = if self.heap.object_record(value).is_some() || self.heap.promise(value).is_some() {
            false
        } else if let Some(array) = self.heap.array(value) {
            // An arguments object is an ordinary object to IsArray, so it serializes as one.
            !array.is_arguments_object()
        } else {
            match json_cell_form(self.heap.get(value)) {
                JsonForm::BigInt => return Err(self.type_error(p, JSON_BIGINT_MESSAGE.into()).into()),
                JsonForm::Omitted => return Ok(false),
                JsonForm::Object => self.is_array(p, value)?,
            }
        };
        let written = if is_array {
            self.json_write_array(Some(p), value, w)
        } else {
            self.json_write_object(Some(p), value, None, w)
        };
        written.map(|()| true)
    }

    fn json_unbox(&mut self, p: &ResidualProgram, value: Value) -> Result<Value, JsError> {
        use crate::BuiltinBrand as B;
        match self.boxed_primitive_data(value) {
            Some((B::StringObject, _)) => {
                let text = self.coerce_js_string(p, value)?;
                Ok(self.heap.alloc_string(text))
            }
            Some((B::NumberObject, _)) => Ok(Value::number(self.to_number(p, value)?)),
            Some((B::BooleanObject | B::BigIntObject, payload)) => Ok(payload),
            _ => Ok(value),
        }
    }

    fn json_raw_text(&self, value: Value) -> Option<String> {
        if !self.object_data(value).is_some_and(Object::is_raw_json) {
            return None;
        }
        let raw = self.lookup_atom("rawJSON")?;
        self.heap.string(self.own_property(value, raw)?).map(|text| text.host_string().to_owned())
    }

    /// QuoteJSONString of the property name `atom`, written from the atom's own text.
    fn write_json_key(&self, atom: Atom, output: &mut JsonText) {
        match self.atom_text(atom) {
            AtomText::Host(text) => write_json_utf8(text.as_bytes(), output),
            AtomText::Js(text) => write_json_string(text, output),
        }
    }

    /// SerializeJSONObject's K for an object the writer has no plan for: the replacer's
    /// property list, or the object's enumerable own string keys, whose gathering may run
    /// guest code (a Proxy's traps). The steady call stops here.
    fn json_member_keys(
        &mut self,
        p: Option<&ResidualProgram>,
        object: Value,
        w: &mut JsonWriter,
    ) -> Result<super::object_keys::EnumerableKeys<Atom>, JsonStop> {
        if let Some(property_list) = &w.options.property_list {
            return Ok(property_list.iter().copied().collect());
        }
        let keys = self.json_enumerable_keys(p.ok_or(JsonStop::Plain)?, object)?;
        w.after_guest_code(self.json_epoch());
        Ok(keys)
    }

    fn json_enumerable_keys(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<super::object_keys::EnumerableKeys<Atom>, JsError> {
        if let Some(keys) = self.shape_enumerable_keys(object) {
            return Ok(keys);
        }
        self.with_call_roots([object], |vm| {
            let keys = vm.object_own_keys(p, object)?;
            vm.with_call_roots([keys], |vm| {
                let length = vm
                    .heap
                    .array_elements(keys)
                    .map(|elements| elements.len())
                    .expect("OwnPropertyKeys projects an array");
                let mut names = super::object_keys::EnumerableKeys::new();
                for index in 0..length {
                    let key = vm
                        .heap
                        .array_elements(keys)
                        .map(|elements| elements.get(index).expect("rooted own-key projection"))
                        .expect("rooted own-key projection");
                    if !vm.heap.is_string(key) {
                        continue;
                    }
                    let current = vm.with_call_roots([key], |vm| -> Result<(), JsError> {
                        let descriptor = vm.object_get_own_property_descriptor(p, &[object, key])?;
                        if vm.descriptor_flag(descriptor, vm.runtime_atoms.enumerable)
                            && let Some(name) = vm.heap.string(key).cloned()
                        {
                            names.push(vm.intern_js_atom(&name));
                        }
                        Ok(())
                    });
                    current?;
                }
                Ok(names)
            })
        })
    }
}

/// How SerializeJSONValue writes a heap value that no earlier step classified.
enum JsonForm {
    /// An object (an array, if IsArray says so).
    Object,
    /// SerializeJSONValue throws a TypeError.
    BigInt,
    /// No JSON value: a function, or an internal record no guest value is.
    Omitted,
}

/// The `JsonForm` of `cell`. Strings, symbols, arrays and promises are classified by earlier
/// steps; they are listed so the match stays exhaustive.
fn json_cell_form(cell: Option<&Cell>) -> JsonForm {
    use crate::heap::Cell as C;
    match cell {
        Some(C::BigInt(_)) => JsonForm::BigInt,
        Some(
            C::Promise(_) | C::Proxy { .. } | C::Date { .. } | C::Error(_)
            | C::ShadowRealm { .. } | C::WeakMap { .. } | C::WeakSet { .. }
            | C::WeakRef { .. } | C::FinalizationRegistry { .. } | C::RegExp { .. } | C::TypedArray { .. }
            | C::ArrayBuffer { .. } | C::DataView { .. } | C::TemporalDuration { .. }
            | C::TemporalPlainDate { .. } | C::TemporalPlainDateTime { .. } | C::TemporalPlainMonthDay { .. }
            | C::TemporalPlainYearMonth { .. } | C::TemporalZonedDateTime { .. } | C::TemporalInstant { .. }
            | C::WasmObject { .. } | C::WasmGlobal { .. } | C::WasmMemory { .. } | C::WasmTable { .. },
        ) => JsonForm::Object,
        None
        | Some(
            C::ArrayFromAsyncState(_) | C::AwaitState(_)
            | C::PromiseResolvingState { .. } | C::WasmElements(_) | C::WasmBits64(_) | C::WasmInstance(_)
            | C::WasmExtern(_) | C::AccessorPair { .. } | C::WasmTag { .. }
            | C::WasmException { .. } | C::WasmV128(_) | C::BindingReference { .. }
            | C::WasmHostFunction { .. },
        ) => JsonForm::Omitted,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        JsString, JsonBuilder, JsonLiteral, JsonParser, JsonStringUnits, JsonUnit, Range, Units, WORD_BYTES,
        write_json_string,
    };

    /// Keeps each string's value and the source range of the primitive the parse reports.
    struct PrimitiveSpans(Option<Range<usize>>);

    impl JsonBuilder for PrimitiveSpans {
        type Value = JsString;
        type Key = ();
        type Layout = ();
        const KEEPS_SOURCE: bool = true;
        fn literal(&mut self, _value: JsonLiteral) -> JsString {
            JsString::from_str("")
        }
        fn number(&mut self, _value: f64) -> JsString {
            JsString::from_str("")
        }
        fn integer(&mut self, _value: i32) -> JsString {
            JsString::from_str("")
        }
        fn string(&mut self, units: JsonStringUnits<'_>) -> JsString {
            JsString::from_unit_run(units.units())
        }
        fn layout(&mut self, _parent: Option<&()>) {}
        fn expected_key(&mut self, _layout: &mut (), _matches: impl Fn(&[u8]) -> bool) -> Option<((), usize)> {
            None
        }
        fn key(&mut self, _layout: &mut (), _units: Units<'_>) {}
        fn array(&mut self, _values: std::vec::Drain<'_, JsString>) -> JsString {
            JsString::from_str("")
        }
        fn object(&mut self, _layout: (), _keys: std::vec::Drain<'_, ()>, _values: std::vec::Drain<'_, JsString>) -> JsString {
            JsString::from_str("")
        }
        fn primitive(&mut self, value: JsString, source: Range<usize>) -> JsString {
            self.0 = Some(source);
            value
        }
    }

    #[test]
    fn byte_string_runs_end_where_each_byte_scan_ends() {
        // Every byte value at every position of two 16-byte blocks, a word and past them,
        // after every prefix byte class, as the per-unit definition finds it.
        let fill = [b'a', 0x7f, 0x80, 0xff, b' ', b'!'];
        for &prefix in &fill {
            for length in 0..(4 * WORD_BYTES + 3) {
                for position in 0..=length {
                    for ending in 0..=u8::MAX {
                        let mut units = vec![prefix; length];
                        if position < length {
                            units[position] = ending;
                        }
                        let expected = units.iter().position(|unit| unit.ends_string_run());
                        assert_eq!(
                            <u8 as JsonUnit>::string_run_length(&units),
                            expected,
                            "prefix {prefix:#x} length {length} position {position} byte {ending:#x}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn wide_string_runs_end_where_each_unit_scan_ends() {
        // Units that end no run before, and a spread of units that do, at every position of
        // three 8-unit blocks and a partial one, as the per-unit definition finds it.
        let fill = [b'a' as u16, 0x00e9, 0x3042, 0xd83e, b' ' as u16, 0x0100];
        let endings: Vec<u16> = (0..=0x7f_u16).chain([0x80, 0xe9, 0x3042, 0xd800, 0xffff]).collect();
        for &prefix in &fill {
            for length in 0..(3 * 8 + 3) {
                for position in 0..=length {
                    for &ending in &endings {
                        let mut units = vec![prefix; length];
                        if position < length {
                            units[position] = ending;
                        }
                        let expected = units.iter().position(|unit| unit.ends_string_run());
                        assert_eq!(
                            <u16 as JsonUnit>::string_run_length(&units),
                            expected,
                            "prefix {prefix:#x} length {length} position {position} unit {ending:#x}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn json_string_writer_preserves_lone_surrogates() {
        let value = JsString::from_units(&[0xD800, b'a' as u16, 0xDC00]);
        let mut output = Vec::new();
        write_json_string(&value, &mut output);
        assert_eq!(output, b"\"\\ud800a\\udc00\"");
    }

    #[test]
    fn json_string_writer_keeps_surrogate_pairs_as_scalars() {
        let value = JsString::from_units(&[0xD83E, 0xDD80]);
        let mut output = Vec::new();
        write_json_string(&value, &mut output);
        assert_eq!(output, "\"🦀\"".as_bytes());
    }

    #[test]
    fn json_parser_keeps_escaped_surrogate_units() {
        let source = r#""\ud800a\udc00""#;
        let units = source.encode_utf16().collect::<Vec<_>>();
        let mut spans = PrimitiveSpans(None);
        let value = JsonParser::new(&units, &mut spans).parse().unwrap();
        let source = spans.0.expect("the primitive's source range");
        assert_eq!(Units::Wide(&units[..]).slice(source), units);
        assert_eq!(value.units(), &[0xD800, b'a' as u16, 0xDC00]);
    }
}
