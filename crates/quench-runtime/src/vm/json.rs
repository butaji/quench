use super::object_descriptors::PropertyDescriptorRecord;
use super::atom_keys::AtomText;
use super::wtf16::{JsString, Units};
use super::*;
use crate::unicode;
use std::hash::BuildHasher as _;
use std::io::Write as _;
use std::ops::Range;

const JSON_NULL_INITIAL: u16 = b'n' as u16;
const JSON_TRUE_INITIAL: u16 = b't' as u16;
const JSON_FALSE_INITIAL: u16 = b'f' as u16;
const JSON_QUOTE: u16 = b'"' as u16;
const JSON_ESCAPE: u16 = b'\\' as u16;
const JSON_ARRAY_START: u16 = b'[' as u16;
const JSON_OBJECT_START: u16 = b'{' as u16;
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
const JSON_WHITESPACE: [u16; 4] = [b' ' as u16, b'\t' as u16, b'\n' as u16, b'\r' as u16];
/// `JSON_WHITESPACE` as a bit per unit value, every one of which is below `u64::BITS`.
const JSON_WHITESPACE_MASK: u64 = {
    let mut mask = 0;
    let mut index = 0;
    while index < JSON_WHITESPACE.len() {
        assert!((JSON_WHITESPACE[index] as u32) < u64::BITS);
        mask |= 1 << JSON_WHITESPACE[index];
        index += 1;
    }
    mask
};

/// Whether byte runs `a` and `b` are equal, compared inline a word at a time: the runs a
/// parse compares, keys and repeated strings, are mostly a few words long.
#[inline(always)]
fn same_ascii(a: &[u8], b: &[u8]) -> bool {
    let word = |bytes: &[u8], at: usize| u64::from_le_bytes(bytes[at..at + WORD_BYTES].try_into().expect("a word"));
    let length = a.len();
    if length != b.len() {
        return false;
    }
    if length < WORD_BYTES {
        return short_run_ends(a) == short_run_ends(b);
    }
    let mut at = 0;
    while at + WORD_BYTES < length {
        if word(a, at) != word(b, at) {
            return false;
        }
        at += WORD_BYTES;
    }
    // The last word ends at the end, overlapping the one before when the length is not a
    // multiple of a word.
    word(a, length - WORD_BYTES) == word(b, length - WORD_BYTES)
}

/// The length of the run of `units` no byte ends (`ends_string_run`), from whole 16-byte
/// blocks: `Ok` when a block holds its end, else `Err` of the bytes the blocks covered.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn sse2_run_blocks(units: &[u8]) -> Result<usize, usize> {
    use std::arch::x86_64::{
        __m128i, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_min_epu8, _mm_movemask_epi8, _mm_or_si128, _mm_set1_epi8,
    };
    const BLOCK_BYTES: usize = size_of::<__m128i>();
    let mut offset = 0;
    // SAFETY: SSE2 is part of the x86_64 baseline, and each load reads a whole block of
    // `units`.
    unsafe {
        let quote = _mm_set1_epi8(JSON_QUOTE as i8);
        let escape = _mm_set1_epi8(JSON_ESCAPE as i8);
        let last_control = _mm_set1_epi8(LAST_CONTROL_CHARACTER as i8);
        while offset + BLOCK_BYTES <= units.len() {
            let block = _mm_loadu_si128(units.as_ptr().add(offset).cast());
            // A byte is a control byte exactly when its unsigned minimum with the last one is
            // itself.
            let control = _mm_cmpeq_epi8(_mm_min_epu8(block, last_control), block);
            let ends = _mm_or_si128(_mm_or_si128(_mm_cmpeq_epi8(block, quote), _mm_cmpeq_epi8(block, escape)), control);
            let mask = _mm_movemask_epi8(ends) as u32;
            if mask != 0 {
                return Ok(offset + mask.trailing_zeros() as usize);
            }
            offset += BLOCK_BYTES;
        }
    }
    Err(offset)
}

/// Whether `unit` is one of `JSON_WHITESPACE`.
#[inline(always)]
fn is_json_whitespace(unit: u16) -> bool {
    u32::from(unit) < u64::BITS && JSON_WHITESPACE_MASK >> unit & 1 != 0
}

enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(JsString),
    Source(Box<JsonValue>, JsString),
    Array(Vec<JsonValue>),
    Object(Vec<(JsString, JsonValue)>),
}

struct JsonSerialization {
    replacer: Option<Value>,
    property_list: Option<Vec<Atom>>,
    gap: String,
    ancestors: Vec<Value>,
    to_json: Atom,
    /// The text written so far, and the nesting depth of the value being written.
    output: JsonText,
    depth: usize,
    /// Member plans of the plain objects serialized so far, by shape and prototype
    /// (`Vm::json_object_plan`). They are shape facts, so they hold only while no guest code
    /// runs: the property steps (`Vm::json_serialize_property`) drop them all.
    plans: rustc_hash::FxHashMap<(u32, Value), Option<JsonObjectPlan>>,
    /// Counts the property steps taken, each of which drops `plans`: a plan read under one
    /// count holds until it changes.
    steps: u64,
}

/// An object's enumerable own data members, when it is an ordinary object of a
/// non-dictionary shape that no `toJSON` reaches: serializing it reads each slot, which runs
/// no guest code.
type JsonObjectPlan = std::rc::Rc<[JsonPlanMember]>;

/// One member of a `JsonObjectPlan`: its key, the slot holding its value, and the key as
/// QuoteJSONString writes it.
struct JsonPlanMember {
    key: Atom,
    slot: usize,
    quoted: Box<[u8]>,
}

impl JsonSerialization {
    /// Starts a member of an array or object that already holds `written` members: the
    /// separator, then the line break and indentation a gap asks for.
    fn begin_member(&mut self, written: usize) {
        if written != 0 {
            self.output.push(b',');
        }
        if !self.gap.is_empty() {
            self.output.push(b'\n');
            write_indent(&mut self.output, &self.gap, self.depth);
        }
    }

    /// Closes an array or object of `written` members with `close`.
    fn end_container(&mut self, written: usize, close: u8) {
        if written != 0 && !self.gap.is_empty() {
            self.output.push(b'\n');
            write_indent(&mut self.output, &self.gap, self.depth);
        }
        self.output.push(close);
    }
}

/// The key SerializeJSONProperty reads from its holder: an array index, or a property name.
/// Its string value is allocated only for the `toJSON` and replacer calls that receive it.
#[derive(Clone, Copy)]
enum JsonKey {
    Index(usize),
    Name(Atom),
}

/// The nesting the plain writer (`Vm::json_write_plain`) recurses through without counting
/// stack; deeper data takes the general steps, which do. Request and response bodies nest a
/// few levels.
const JSON_PLAIN_DEPTH: usize = 64;

/// The containers being written, inline up to this nesting.
const JSON_PLAIN_INLINE_ANCESTORS: usize = 16;

/// The state of one plain stringification: no guest code runs during it, so what a shape
/// and prototype decide (lacking `toJSON`, an object's members) holds for its whole run.
struct JsonPlainWrite {
    to_json: Atom,
    ancestors: smallvec::SmallVec<[Value; JSON_PLAIN_INLINE_ANCESTORS]>,
    array_lacks_to_json: Option<(u32, Value)>,
    output: JsonText,
    /// The members of each ordinary object's (shape, prototype) written so far, or `None`
    /// when objects of it take the general serializer.
    object_plans: rustc_hash::FxHashMap<(u32, Value), Option<std::rc::Rc<[JsonMember]>>>,
    /// The `"key":` text of every planned member: an object's keys recur across the objects
    /// of a payload, so each shape's are quoted once.
    key_text: JsonText,
}

/// One enumerable data member of a planned object: its slot and its `"key":` text's range in
/// `JsonPlainWrite::key_text`.
#[derive(Clone, Copy)]
struct JsonMember {
    slot: u32,
    key: (u32, u32),
}

impl JsonPlainWrite {
    fn new(to_json: Atom) -> Self {
        Self {
            to_json,
            ancestors: Default::default(),
            array_lacks_to_json: None,
            output: JsonText::new(),
            object_plans: Default::default(),
            key_text: JsonText::new(),
        }
    }
}

/// An integer's decimal digits, as Number::toString writes them.
fn write_json_integer(value: i32, output: &mut JsonText) {
    output.extend_from_slice(itoa::Buffer::new().format(value).as_bytes());
}

/// The JSON text a stringification writes: UTF-8 bytes, decoded once when it completes.
type JsonText = Vec<u8>;

/// A completed JSON text as a string. Every writer appends UTF-8: ASCII punctuation,
/// literals and numbers, host strings, and JavaScript strings whose lone surrogates it
/// escapes (`write_json_string`).
/// The most text capacity `Vm::json_output` keeps between calls: enough for a multi-megabyte
/// payload's serialization, bounded so one huge result does not stay resident.
const JSON_RETAINED_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

fn json_text_str(text: &[u8]) -> &str {
    debug_assert!(std::str::from_utf8(text).is_ok(), "JSON text is written as UTF-8");
    // SAFETY: as above, every byte sequence written is UTF-8.
    unsafe { std::str::from_utf8_unchecked(text) }
}

fn json_text_string(text: JsonText) -> String {
    debug_assert!(std::str::from_utf8(&text).is_ok(), "JSON text is written as UTF-8");
    // SAFETY: as above, every byte sequence written is UTF-8.
    unsafe { String::from_utf8_unchecked(text) }
}

fn write_json_string(value: &JsString, output: &mut JsonText) {
    match value.units() {
        Units::Ascii(bytes) => write_json_utf8(bytes, output),
        Units::Wide(units) => {
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
    }
}

/// QuoteJSONString of UTF-8 `text`. Every byte QuoteJSONString escapes is ASCII and no
/// multi-byte sequence contains one, so the runs between escapes are copied whole.
fn write_json_utf8(text: &[u8], output: &mut JsonText) {
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
                output.extend_from_slice(&text[run..index + offset]);
                write_json_scalar(char::from(*byte), output);
                run = index + offset + 1;
            }
        }
        index = end;
    }
    output.extend_from_slice(&text[run..]);
    output.push(b'"');
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
            return output.extend_from_slice(scalar.encode_utf8(&mut buffer).as_bytes());
        }
    };
    output.extend_from_slice(escape);
}

/// The `\uXXXX` escape of `code`.
fn write_unicode_escape(code: u32, output: &mut JsonText) {
    let _ = write!(output, "\\u{code:04x}");
}

fn write_indent(output: &mut JsonText, gap: &str, depth: usize) {
    for _ in 0..depth {
        output.extend_from_slice(gap.as_bytes());
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

/// Units V8 quotes on each side of an unexpected token (`JsonParser::kMaxContextCharacters`).
const JSON_ERROR_CONTEXT_UNITS: usize = 10;
/// Sources shorter than this are quoted whole (`kMinOriginalSourceLengthForContext`).
const JSON_ERROR_CONTEXT_MIN_SOURCE: usize = 2 * JSON_ERROR_CONTEXT_UNITS + 1;
/// Sources V8 names whole instead of quoting a token of (`JsonParser::IsSpecialString`):
/// what `undefined`, `NaN`, `Infinity` and a plain object convert to.
const JSON_ERROR_SPECIAL_SOURCES: [&str; 4] = ["[object Object]", "undefined", "Infinity", "NaN"];
/// The last unit an escape may name and be a bad escape rather than an unexpected token
/// (`unibrow::Latin1::kMaxChar`).
const LATIN1_MAX: u16 = 0xFF;
const CARRIAGE_RETURN: u16 = b'\r' as u16;
const LINE_FEED: u16 = b'\n' as u16;

/// The message V8 gives `syntax` at `position` of `source`, its UTF-16 units.
fn json_syntax_message(source: &[u16], syntax: JsonSyntax, position: usize) -> String {
    let unexpected = syntax == JsonSyntax::Unexpected
        || syntax == JsonSyntax::BadEscape && source.get(position).is_some_and(|&unit| unit > LATIN1_MAX);
    let message = if unexpected {
        match source.get(position) {
            None => return "Unexpected end of JSON input".into(),
            Some(&JSON_QUOTE) => "Unexpected string in JSON",
            Some(&(JSON_MINUS | JSON_DIGIT_START..=JSON_DIGIT_END)) => "Unexpected number in JSON",
            Some(&unit) => return json_unexpected_token_message(source, unit, position),
        }
    } else {
        syntax.positioned_message()
    };
    let (line, column) = json_line_column(&source[..position]);
    format!("{message} at position {position} (line {line} column {column})")
}

/// `Unexpected token` of `unit` at `position`, quoting the source around it
/// (`JsonParser::GetErrorMessageWithEllipses`).
fn json_unexpected_token_message(source: &[u16], unit: u16, position: usize) -> String {
    let whole = String::from_utf16_lossy(source);
    if JSON_ERROR_SPECIAL_SOURCES.contains(&whole.as_str()) {
        return format!("\"{whole}\" is not valid JSON");
    }
    let token = String::from_utf16_lossy(&[unit]);
    if source.len() < JSON_ERROR_CONTEXT_MIN_SOURCE {
        return format!("Unexpected token '{token}', \"{whole}\" is not valid JSON");
    }
    let before = position >= JSON_ERROR_CONTEXT_UNITS;
    let after = !before || position + JSON_ERROR_CONTEXT_UNITS < source.len();
    let start = if before { position - JSON_ERROR_CONTEXT_UNITS } else { 0 };
    let end = if after { position + JSON_ERROR_CONTEXT_UNITS } else { source.len() };
    let context = String::from_utf16_lossy(&source[start..end]);
    let (open, close) = (if before { "..." } else { "" }, if after { "..." } else { "" });
    format!("Unexpected token '{token}', {open}\"{context}\"{close} is not valid JSON")
}

/// The one-based line and column after `prefix`: JSON's line terminators are CR and LF, and
/// CR LF is one (`JsonParser::CalculateFileLocation`).
fn json_line_column(prefix: &[u16]) -> (usize, usize) {
    let mut line = 1;
    let mut line_start = 0;
    let mut index = 0;
    while index < prefix.len() {
        if prefix[index] == CARRIAGE_RETURN && index + 1 < prefix.len() && prefix[index + 1] == LINE_FEED {
            index += 1;
        }
        if prefix[index] == CARRIAGE_RETURN || prefix[index] == LINE_FEED {
            line += 1;
            line_start = index + 1;
        }
        index += 1;
    }
    (line, 1 + prefix.len() - line_start)
}

/// Number literals at most this long parse from a stack buffer.
const JSON_INLINE_NUMBER_LENGTH: usize = 64;

/// Integer literals of at most this many digits are below 2^53, so accumulating their
/// digits gives the exact value the decimal conversion would.
const JSON_EXACT_INTEGER_DIGITS: usize = 15;
/// Decimal digits a `u64` accumulates without overflow.
const JSON_MANTISSA_DIGITS: usize = 19;
/// The integers up to this are all exact doubles.
const EXACT_DOUBLE_INTEGER: u64 = 1 << f64::MANTISSA_DIGITS;
/// The powers of ten that are exact doubles: 10^22 is the last, as 5^22 < 2^53 < 5^23.
const EXACT_POWERS_OF_TEN: [f64; 23] = {
    let mut powers = [1.0; 23];
    let mut index = 1;
    while index < powers.len() {
        powers[index] = powers[index - 1] * 10.0;
        index += 1;
    }
    powers
};

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
    /// The units of `units` before the first that `ends_string_run`; `None` when none does.
    #[inline(always)]
    fn string_run_length(units: &[Self]) -> Option<usize> {
        units.iter().position(|unit| unit.ends_string_run())
    }
    /// Whether `units` start with `key`, an ASCII key whose text needs no escape, and then
    /// the quote that closes it: the source of a string whose units are `key`.
    #[inline(always)]
    fn starts_key(units: &[Self], key: &[u8]) -> bool {
        units.get(key.len()).is_some_and(|unit| unit.unit() == JSON_QUOTE)
            && units.iter().zip(key).all(|(unit, &byte)| unit.unit() == u16::from(byte))
    }
    /// `units` as a run of string units.
    fn run(units: &[Self]) -> JsonStringUnits<'_>;
    /// Appends the string units of `units`, a run no escape, quote or control unit ends.
    fn extend_units(units: &[Self], output: &mut Vec<u16>);
}

impl JsonUnit for u8 {
    fn unit(self) -> u16 {
        u16::from(self)
    }
    #[inline(always)]
    fn ends_string_run(self) -> bool {
        STRING_RUN_ENDS[usize::from(self)]
    }
    /// Sixteen bytes at a time where SSE2 is the baseline (`sse2_run_blocks`), then a word
    /// at a time (`json_escape_bytes`, the bytes that end a run), then the rest one by one.
    #[inline(always)]
    fn string_run_length(units: &[u8]) -> Option<usize> {
        #[cfg(target_arch = "x86_64")]
        let offset = match sse2_run_blocks(units) {
            Ok(length) => return Some(length),
            Err(scanned) => scanned,
        };
        #[cfg(not(target_arch = "x86_64"))]
        let offset = 0;
        let (words, remainder) = units[offset..].as_chunks::<WORD_BYTES>();
        let mut offset = offset;
        for word in words {
            let word = u64::from_le_bytes(*word);
            let ends = json_escape_bytes(word);
            if ends != 0 {
                return Some(offset + (ends.trailing_zeros() / u8::BITS) as usize);
            }
            offset += WORD_BYTES;
        }
        remainder.iter().position(|unit| unit.ends_string_run()).map(|position| offset + position)
    }
    #[inline(always)]
    fn starts_key(units: &[u8], key: &[u8]) -> bool {
        units.get(key.len()) == Some(&b'"') && same_ascii(&units[..key.len()], key)
    }
    fn run(units: &[u8]) -> JsonStringUnits<'_> {
        JsonStringUnits::Run(Units::Ascii(units))
    }
    fn extend_units(units: &[u8], output: &mut Vec<u16>) {
        output.extend(units.iter().map(|&unit| u16::from(unit)));
    }
}

impl JsonUnit for u16 {
    fn unit(self) -> u16 {
        self
    }
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

    /// The text of a run of whole UTF-8 characters.
    fn text(units: &[Self]) -> &str {
        // SAFETY: `Utf8Byte` is a transparent `u8`.
        let bytes = unsafe { std::slice::from_raw_parts(units.as_ptr().cast::<u8>(), units.len()) };
        std::str::from_utf8(bytes).expect("a run bounded by ASCII terminators is whole UTF-8 text")
    }
}

impl JsonUnit for Utf8Byte {
    fn unit(self) -> u16 {
        u16::from(self.0)
    }
    #[inline(always)]
    fn ends_string_run(self) -> bool {
        STRING_RUN_ENDS[usize::from(self.0)]
    }
    #[inline(always)]
    fn string_run_length(units: &[Self]) -> Option<usize> {
        // SAFETY: `Utf8Byte` is a transparent `u8`.
        u8::string_run_length(unsafe { std::slice::from_raw_parts(units.as_ptr().cast::<u8>(), units.len()) })
    }
    #[inline(always)]
    fn starts_key(units: &[Self], key: &[u8]) -> bool {
        // SAFETY: `Utf8Byte` is a transparent `u8`.
        u8::starts_key(unsafe { std::slice::from_raw_parts(units.as_ptr().cast::<u8>(), units.len()) }, key)
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

/// Parses `text` with `builder`, in the form the string holds: an ASCII string's bytes need
/// no widening, and decoded text whose units are not made parses from its UTF-8, whose
/// error positions are then counted in UTF-16 units.
fn parse_json_text<B: JsonBuilder>(text: &JsString, builder: &mut B) -> Result<B::Value, JsonParseError> {
    if let Some(host) = text.unflattened_host() {
        return JsonParser::new(Utf8Byte::of(host), builder).parse().map_err(|error| JsonParseError {
            position: String::from_utf8_lossy(&host.as_bytes()[..error.position]).encode_utf16().count(),
            ..error
        });
    }
    match text.units() {
        Units::Ascii(bytes) => JsonParser::new(bytes, builder).parse(),
        Units::Wide(units) => JsonParser::new(units, builder).parse(),
    }
}

/// What a parse makes of each JSON value: the source-bearing `JsonValue` tree a reviver
/// walks, or heap values directly when nothing reads the source afterwards.
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
    /// A primitive parsed from `source`, the text a reviver's context exposes.
    fn primitive(&mut self, value: Self::Value, source: Units<'_>) -> Self::Value;
}

#[derive(Clone, Copy)]
enum JsonLiteral {
    Null,
    True,
    False,
}

/// Builds the `JsonValue` tree, each primitive with its source text.
struct JsonTreeBuilder;

impl JsonBuilder for JsonTreeBuilder {
    type Value = JsonValue;
    type Key = JsString;
    type Layout = ();
    const KEEPS_SOURCE: bool = true;
    fn literal(&mut self, value: JsonLiteral) -> JsonValue {
        match value {
            JsonLiteral::Null => JsonValue::Null,
            JsonLiteral::True => JsonValue::Bool(true),
            JsonLiteral::False => JsonValue::Bool(false),
        }
    }
    fn number(&mut self, value: f64) -> JsonValue {
        JsonValue::Number(value)
    }
    fn integer(&mut self, value: i32) -> JsonValue {
        JsonValue::Number(f64::from(value))
    }
    fn string(&mut self, units: JsonStringUnits<'_>) -> JsonValue {
        JsonValue::String(JsString::from_unit_run(units.units()))
    }
    fn layout(&mut self, _parent: Option<&()>) {}
    fn expected_key(&mut self, _layout: &mut (), _matches: impl Fn(&[u8]) -> bool) -> Option<(JsString, usize)> {
        None
    }
    fn key(&mut self, _layout: &mut (), units: Units<'_>) -> JsString {
        JsString::from_unit_run(units)
    }
    fn array(&mut self, values: std::vec::Drain<'_, JsonValue>) -> JsonValue {
        JsonValue::Array(values.collect())
    }
    fn object(
        &mut self,
        _layout: (),
        keys: std::vec::Drain<'_, JsString>,
        values: std::vec::Drain<'_, JsonValue>,
    ) -> JsonValue {
        JsonValue::Object(keys.zip(values).collect())
    }
    fn primitive(&mut self, value: JsonValue, source: Units<'_>) -> JsonValue {
        JsonValue::Source(Box::new(value), JsString::from_unit_run(source))
    }
}

/// A layout a parse met: the shape objects whose keys led here take, and the edge its latest
/// object went on along, which the next object of the layout is expected to take (V8's
/// expected transition, `JsonParser::ScanJsonPropertyKey`).
struct JsonLayout {
    shape: u32,
    expected: u32,
}

/// A key added to a layout: the bytes of the key's text when it is ASCII and needs no
/// escape (`key_bytes`), which the source is compared with in place of scanning its key, its
/// atom, and the layout the add leads to.
struct JsonLayoutEdge {
    key: Range<u32>,
    atom: Atom,
    to: u32,
}

/// Where an object's keys have led so far: its layout, `JSON_UNPLANNED` once a key repeats;
/// the edge its latest key took; and the context its first key is expected from
/// (`JsonHeapBuilder::first_keys`).
#[derive(Clone, Copy)]
struct JsonObjectLayout {
    layout: u32,
    last: u32,
    context: u32,
}

/// The layout of an object whose keys repeat one, which therefore needs a property store
/// per key rather than one shape (`JsonHeapBuilder::object`); and an edge not taken yet.
const JSON_UNPLANNED: u32 = u32::MAX;
/// The layout of an object before its first key.
const JSON_EMPTY_LAYOUT: u32 = 0;
/// The context of an object that is no member's value, nor within one.
const JSON_ROOT_CONTEXT: u32 = 0;

/// Builds heap values as the parse goes. The heap does not collect inside a native call, so
/// values gathered for an array or object stay valid until their container holds them, and
/// the shapes and string values below stay valid for the parse.
struct JsonHeapBuilder<'v, H: Host> {
    vm: &'v mut Vm<H>,
    array_prototype: Value,
    object_prototype: Value,
    /// The first property store that failed; the parse finishes and then reports it.
    error: Option<JsError>,
    /// The layouts the parse met, `JSON_EMPTY_LAYOUT` first, and the keys added to them.
    layouts: Vec<JsonLayout>,
    edges: Vec<JsonLayoutEdge>,
    /// The first edge the latest object of each context took: objects that are the value
    /// of one member, directly or within arrays, mostly share a layout, as V8's parse expects
    /// of an array's elements. The context of an object under a member is one past the
    /// edge of that member's key; `JSON_ROOT_CONTEXT` precedes them.
    first_keys: Vec<u32>,
    /// The edge of each (layout, key) add, for a key other than the one expected.
    edge_index: rustc_hash::FxHashMap<(u32, Atom), u32>,
    /// The ASCII text of each edge's key.
    key_bytes: Vec<u8>,
    /// The atom of each ASCII key the parse met outside the expected edges: a JSON text
    /// repeats a few keys, which this hashes a word at a time instead of interning unit by
    /// unit (`intern_unit_run_atom`). Atoms outlive every collection.
    ascii_keys: rustc_hash::FxHashMap<Box<[u8]>, Atom>,
    /// The string value made of each ASCII run of the text the parse met, with the run's
    /// bytes in `string_bytes`: a string has no identity, so a run the text repeats takes
    /// the value made once, as V8 internalizes a JSON text's repeated strings. The bytes are
    /// copied together so that comparing a run reads them from cache rather than from the
    /// run's first place in the text.
    strings: hashbrown::HashTable<(Range<u32>, Value)>,
    string_bytes: Vec<u8>,
}

enum JsonBuildError {
    Parse(JsonParseError),
    Runtime(JsError),
}

/// A hash of ASCII run `bytes` from its length and its first and last words: runs the
/// table holds are compared whole, so the hash need not read every byte.
#[inline(always)]
fn ascii_run_hash(bytes: &[u8]) -> u64 {
    let ends = if bytes.len() < WORD_BYTES {
        short_run_ends(bytes)
    } else {
        let word = |at: usize| u64::from_le_bytes(bytes[at..at + WORD_BYTES].try_into().expect("a word"));
        (word(0), word(bytes.len() - WORD_BYTES))
    };
    rustc_hash::FxBuildHasher.hash_one((bytes.len(), ends))
}

/// The bytes of a run shorter than a word, as two values that together with its length
/// determine it, read without a loop (as wyhash reads short keys): its first and last four
/// bytes, which overlap below eight, else its first, middle and last bytes.
#[inline(always)]
fn short_run_ends(bytes: &[u8]) -> (u64, u64) {
    const HALF_WORD: usize = WORD_BYTES / 2;
    let length = bytes.len();
    let half = |at: usize| u64::from(u32::from_le_bytes(bytes[at..at + HALF_WORD].try_into().expect("half a word")));
    if length >= HALF_WORD {
        (half(0), half(length - HALF_WORD))
    } else if length > 0 {
        let byte = |at: usize| u64::from(bytes[at]);
        (byte(0) | byte(length / 2) << u8::BITS, byte(length - 1))
    } else {
        (0, 0)
    }
}

impl<'v, H: Host> JsonHeapBuilder<'v, H> {
    fn new(vm: &'v mut Vm<H>) -> Self {
        let globals = vm.realm.globals;
        let object_prototype = vm.realm_object_prototype(globals);
        let empty_shape = Vm::<H>::empty_object(object_prototype).shape();
        Self {
            array_prototype: vm.array_prototype_for_realm(globals),
            object_prototype,
            vm,
            error: None,
            layouts: vec![JsonLayout {
                shape: empty_shape,
                expected: JSON_UNPLANNED,
            }],
            edges: Vec::new(),
            first_keys: vec![JSON_UNPLANNED],
            edge_index: Default::default(),
            key_bytes: Vec::new(),
            ascii_keys: Default::default(),
            strings: hashbrown::HashTable::new(),
            string_bytes: Vec::new(),
        }
    }

    fn finish(self, parsed: Result<Value, JsonParseError>) -> Result<Value, JsonBuildError> {
        let value = parsed.map_err(JsonBuildError::Parse)?;
        match self.error {
            Some(error) => Err(JsonBuildError::Runtime(error)),
            None => Ok(value),
        }
    }

    fn materialize(&mut self, value: &JsonValue) -> Result<Value, JsError> {
        let _stack = self.vm.enter_stack()?;
        Ok(match value {
            JsonValue::Null => self.literal(JsonLiteral::Null),
            JsonValue::Bool(true) => self.literal(JsonLiteral::True),
            JsonValue::Bool(false) => self.literal(JsonLiteral::False),
            JsonValue::Number(value) => self.number(*value),
            JsonValue::String(value) => self.vm.heap.alloc_string(value.clone()),
            JsonValue::Source(value, _) => self.materialize(value)?,
            JsonValue::Array(values) => {
                let mut values = values
                    .iter()
                    .map(|value| self.materialize(value))
                    .collect::<Result<Vec<_>, _>>()?;
                self.array(values.drain(..))
            }
            JsonValue::Object(entries) => {
                let mut layout = self.layout(None);
                let mut keys = Vec::with_capacity(entries.len());
                let mut values = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    let key = self.vm.intern_js_atom(key);
                    self.advance(&mut layout, key);
                    keys.push(key);
                    values.push(self.materialize(value)?);
                }
                self.object(layout, keys.drain(..), values.drain(..))
            }
        })
    }

    /// The atom of key `units`.
    fn atom(&mut self, units: Units<'_>) -> Atom {
        let Units::Ascii(bytes) = units else {
            return self.vm.intern_unit_run_atom(units);
        };
        if let Some(&atom) = self.ascii_keys.get(bytes) {
            return atom;
        }
        let atom = self.vm.intern_unit_run_atom(units);
        self.ascii_keys.insert(bytes.into(), atom);
        atom
    }

    /// The edge `object` expects its next key to take.
    #[inline(always)]
    fn expected_edge(&self, object: &JsonObjectLayout) -> u32 {
        match object.layout {
            JSON_EMPTY_LAYOUT => self.first_keys[object.context as usize],
            layout => self.layouts[layout as usize].expected,
        }
    }

    /// Moves `object` on along `edge`, which its layout now expects next.
    #[inline(always)]
    fn take(&mut self, object: &mut JsonObjectLayout, edge: u32) {
        match object.layout {
            JSON_EMPTY_LAYOUT => self.first_keys[object.context as usize] = edge,
            layout => self.layouts[layout as usize].expected = edge,
        }
        object.layout = self.edges[edge as usize].to;
        object.last = edge;
    }

    /// Moves `object` on by an add of `atom`: along the edge this parse made for it, else
    /// along a new one. A key the layout holds already leaves the object unplanned.
    fn advance(&mut self, object: &mut JsonObjectLayout, atom: Atom) {
        let layout = object.layout;
        if layout == JSON_UNPLANNED {
            return;
        }
        if let Some(&edge) = self.edge_index.get(&(layout, atom)) {
            return self.take(object, edge);
        }
        let shape = self.layouts[layout as usize].shape;
        if self.vm.property_shape_slot(shape, PropertyKey::string(atom)).is_some() {
            object.layout = JSON_UNPLANNED;
            return;
        }
        let next = self.vm.transition_shape(shape, atom);
        let to = if self.vm.shape_is_dictionary(next) {
            JSON_UNPLANNED
        } else {
            self.layouts.push(JsonLayout {
                shape: next,
                expected: JSON_UNPLANNED,
            });
            (self.layouts.len() - 1) as u32
        };
        // A key that is not plain ASCII has no source text to match: its range is out of
        // bounds.
        let text = self.vm.atom_name(atom);
        let key = if text.bytes().all(|byte| byte.is_ascii() && !STRING_RUN_ENDS[usize::from(byte)]) {
            let start = self.key_bytes.len() as u32;
            self.key_bytes.extend_from_slice(text.as_bytes());
            start..self.key_bytes.len() as u32
        } else {
            JSON_UNPLANNED..JSON_UNPLANNED
        };
        self.edges.push(JsonLayoutEdge { key, atom, to });
        self.first_keys.push(JSON_UNPLANNED);
        let edge = (self.edges.len() - 1) as u32;
        self.edge_index.insert((layout, atom), edge);
        self.take(object, edge);
    }

    /// An object of keys one repeats: each key stored in turn, as CreateDataProperty does,
    /// so a repeated key keeps its first position and takes its last value.
    #[cold]
    fn unplanned_object(&mut self, keys: std::vec::Drain<'_, Atom>, values: std::vec::Drain<'_, Value>) -> Value {
        let object = self
            .vm
            .heap
            .alloc(Cell::Object(Vm::<H>::empty_object(self.object_prototype)));
        for (atom, value) in keys.zip(values) {
            let key = PropertyKey::string(atom);
            let stored = match self.vm.object_data(object).map(|data| data.shape()) {
                Some(shape) if self.vm.property_shape_slot(shape, key).is_none() => {
                    self.vm.create_shape_data_property(object, key, value, DEFAULT_DATA_ATTRIBUTES)
                }
                _ => self.vm.set_property(object, atom, value),
            };
            if let Err(error) = stored {
                self.error.get_or_insert(error);
            }
        }
        object
    }

    /// The string value of ASCII run `bytes`: the one made for the same run before.
    fn ascii_string(&mut self, bytes: &[u8]) -> Value {
        let hash = ascii_run_hash(bytes);
        let text = |range: &Range<u32>| &self.string_bytes[range.start as usize..range.end as usize];
        if let Some((_, value)) = self.strings.find(hash, |(range, _)| same_ascii(text(range), bytes)) {
            return *value;
        }
        let value = self.vm.heap.alloc_string(JsString::from_unit_run(Units::Ascii(bytes)));
        let start = self.string_bytes.len() as u32;
        self.string_bytes.extend_from_slice(bytes);
        let range = start..self.string_bytes.len() as u32;
        let string_bytes = &self.string_bytes;
        self.strings.insert_unique(hash, (range, value), |(range, _)| {
            ascii_run_hash(&string_bytes[range.start as usize..range.end as usize])
        });
        value
    }
}

impl<H: Host> JsonBuilder for JsonHeapBuilder<'_, H> {
    type Value = Value;
    type Key = Atom;
    type Layout = JsonObjectLayout;
    const KEEPS_SOURCE: bool = false;
    fn literal(&mut self, value: JsonLiteral) -> Value {
        match value {
            JsonLiteral::Null => Value::NULL,
            JsonLiteral::True => Value::TRUE,
            JsonLiteral::False => Value::FALSE,
        }
    }
    fn number(&mut self, value: f64) -> Value {
        Value::number(value)
    }
    fn integer(&mut self, value: i32) -> Value {
        Value::integer(value)
    }
    #[inline(always)]
    fn string(&mut self, units: JsonStringUnits<'_>) -> Value {
        match units {
            JsonStringUnits::Run(Units::Ascii(bytes)) => self.ascii_string(bytes),
            units => self.vm.heap.alloc_string(JsString::from_unit_run(units.units())),
        }
    }
    fn layout(&mut self, parent: Option<&JsonObjectLayout>) -> JsonObjectLayout {
        let context = match parent {
            Some(parent) if parent.last != JSON_UNPLANNED => parent.last + 1,
            _ => JSON_ROOT_CONTEXT,
        };
        JsonObjectLayout {
            layout: JSON_EMPTY_LAYOUT,
            last: JSON_UNPLANNED,
            context,
        }
    }
    #[inline(always)]
    fn expected_key(&mut self, object: &mut JsonObjectLayout, matches: impl Fn(&[u8]) -> bool) -> Option<(Atom, usize)> {
        if object.layout == JSON_UNPLANNED {
            return None;
        }
        let edge = self.expected_edge(object);
        let entry = self.edges.get(edge as usize)?;
        let key = self.key_bytes.get(entry.key.start as usize..entry.key.end as usize)?;
        if !matches(key) {
            return None;
        }
        let found = (entry.atom, key.len());
        self.take(object, edge);
        Some(found)
    }
    fn key(&mut self, object: &mut JsonObjectLayout, units: Units<'_>) -> Atom {
        let atom = self.atom(units);
        self.advance(object, atom);
        atom
    }
    fn array(&mut self, values: std::vec::Drain<'_, Value>) -> Value {
        let values = values.as_slice();
        self.vm
            .heap
            .alloc_literal_array(self.array_prototype, values.len(), |index| values[index])
    }
    fn object(
        &mut self,
        object: JsonObjectLayout,
        keys: std::vec::Drain<'_, Atom>,
        values: std::vec::Drain<'_, Value>,
    ) -> Value {
        if object.layout == JSON_UNPLANNED {
            return self.unplanned_object(keys, values);
        }
        let shape = self.layouts[object.layout as usize].shape;
        self.vm
            .heap
            .alloc_object_with_properties(self.object_prototype, shape, values.as_slice())
    }
    fn primitive(&mut self, value: Value, _source: Units<'_>) -> Value {
        value
    }
}

/// The key an object literal takes as its prototype rather than as a property
/// (`__proto__: value`), which JSON.parse takes as a property.
const PROTO_KEY: &str = "__proto__";

/// Checks the JSON text of a data script's literal (`compile::data_literal`), keeping no
/// values: the text is an array or object literal with the value JSON.parse gives it unless
/// a key is `PROTO_KEY`.
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
    fn primitive(&mut self, _value: (), _source: Units<'_>) {}
}

/// The bytes of the JSON array or object text `text` starts with, when the array or object
/// literal of that text evaluates to the value JSON.parse gives it: no key is `PROTO_KEY`.
pub(crate) fn json_literal_length(text: &str) -> Option<usize> {
    if !text.starts_with(['[', '{']) {
        return None;
    }
    let mut check = JsonLiteralCheck { proto_key: false };
    let (_, length) = JsonParser::new(Utf8Byte::of(text), &mut check).parse_prefix().ok()?;
    (!check.proto_key).then_some(length)
}

/// A container whose members a parse is gathering: where its members start on the parse's
/// stacks, and an object's layout so far.
enum JsonFrame<L> {
    Array { values: usize },
    Object { values: usize, keys: usize, layout: L },
}

/// A parse of `units` into `builder`'s values, iterative as V8's `JsonParser` is, so nesting
/// is bounded by memory rather than the native stack. Containers gather their members on
/// two stacks the whole parse shares, so each takes exactly its members' storage once.
struct JsonParser<'a, 'b, T, B: JsonBuilder> {
    units: &'a [T],
    index: usize,
    builder: &'b mut B,
    values: Vec<B::Value>,
    keys: Vec<B::Key>,
}

impl<'a, 'b, T: JsonUnit, B: JsonBuilder> JsonParser<'a, 'b, T, B> {
    fn new(units: &'a [T], builder: &'b mut B) -> Self {
        Self {
            units,
            index: 0,
            builder,
            values: Vec::new(),
            keys: Vec::new(),
        }
    }

    /// The JSON text that is all of the units.
    fn parse(mut self) -> Result<B::Value, JsonParseError> {
        let value = self.value()?;
        match self.token() {
            None => Ok(value),
            Some(_) => Err(self.error(JsonSyntax::NonWhitespaceAfter)),
        }
    }

    /// The JSON text the units start with, and the units it spans.
    fn parse_prefix(mut self) -> Result<(B::Value, usize), JsonParseError> {
        let value = self.value()?;
        Ok((value, self.index))
    }

    /// The JSON value at the current unit, which the parse moves past.
    fn value(&mut self) -> Result<B::Value, JsonParseError> {
        let mut frames: Vec<JsonFrame<B::Layout>> = Vec::new();
        'value: loop {
            let token = self.token();
            let start = self.index;
            let mut value = match token {
                Some(JSON_QUOTE) => {
                    let text = self.string()?;
                    self.builder.string(text)
                }
                Some(JSON_MINUS | JSON_DIGIT_START..=JSON_DIGIT_END) => match self.number()? {
                    JsonNumber::Integer(value) => self.builder.integer(value),
                    JsonNumber::Float(value) => self.builder.number(value),
                },
                Some(JSON_OBJECT_START) => {
                    self.index += 1;
                    let parent = frames.iter().rev().find_map(|frame| match frame {
                        JsonFrame::Object { layout, .. } => Some(layout),
                        JsonFrame::Array { .. } => None,
                    });
                    let layout = self.builder.layout(parent);
                    let (values, keys) = (self.values.len(), self.keys.len());
                    if self.token() == Some(JSON_OBJECT_END) {
                        self.index += 1;
                        self.builder.object(layout, self.keys.drain(keys..), self.values.drain(values..))
                    } else {
                        // The frame holds the layout its keys advance in place: a copy made
                        // right after their stores would read them back through memory.
                        frames.push(JsonFrame::Object { values, keys, layout });
                        let Some(JsonFrame::Object { layout, .. }) = frames.last_mut() else {
                            unreachable!("the frame is an object's")
                        };
                        self.member_key(layout, JsonSyntax::ExpectedPropertyNameOrBrace)?;
                        continue 'value;
                    }
                }
                Some(JSON_ARRAY_START) => {
                    self.index += 1;
                    let values = self.values.len();
                    if self.token() == Some(JSON_ARRAY_END) {
                        self.index += 1;
                        self.builder.array(self.values.drain(values..))
                    } else {
                        frames.push(JsonFrame::Array { values });
                        continue 'value;
                    }
                }
                Some(JSON_NULL_INITIAL) => self.literal(b"null", JsonLiteral::Null)?,
                Some(JSON_TRUE_INITIAL) => self.literal(b"true", JsonLiteral::True)?,
                Some(JSON_FALSE_INITIAL) => self.literal(b"false", JsonLiteral::False)?,
                _ => return Err(self.error(JsonSyntax::Unexpected)),
            };
            if B::KEEPS_SOURCE && !matches!(token, Some(JSON_OBJECT_START | JSON_ARRAY_START)) {
                value = self.builder.primitive(value, T::run(&self.units[start..self.index]).units());
            }
            // Each container the value completes is the value its parent takes next.
            loop {
                let Some(frame) = frames.last_mut() else {
                    return Ok(value);
                };
                let token = self.token();
                self.values.push(value);
                value = match (frame, token) {
                    (JsonFrame::Array { .. }, Some(JSON_COMMA)) => {
                        self.index += 1;
                        continue 'value;
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
                        continue 'value;
                    }
                    (JsonFrame::Object { .. }, Some(JSON_OBJECT_END)) => {
                        self.index += 1;
                        let Some(JsonFrame::Object { values, keys, layout }) = frames.pop() else {
                            unreachable!("the frame is an object's")
                        };
                        self.builder.object(layout, self.keys.drain(keys..), self.values.drain(values..))
                    }
                    (JsonFrame::Object { .. }, _) => return Err(self.error(JsonSyntax::ExpectedCommaOrBrace)),
                };
            }
        }
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
        for &unit in expected {
            if self.peek() != Some(u16::from(unit)) {
                return Err(self.error(JsonSyntax::Unexpected));
            }
            self.index += 1;
        }
        Ok(self.builder.literal(value))
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

    /// A number, whose first unit, a digit or a minus sign, the caller matched. Its digits
    /// accumulate as they are scanned: an integer short enough to be exact is that integer,
    /// and a decimal whose digits and power of ten are exact doubles is their one correctly
    /// rounded product or quotient (Clinger's fast path); any other literal converts in full.
    fn number(&mut self) -> Result<JsonNumber, JsonParseError> {
        let start = self.index;
        let negative = self.take_if(b'-');
        let integer_start = self.index;
        let mut mantissa = 0_u64;
        match self.peek() {
            Some(JSON_DIGIT_START) => {
                self.index += 1;
                if self.digit_value().is_some() {
                    return Err(self.error(JsonSyntax::Unexpected));
                }
            }
            Some(JSON_DIGIT_START..=JSON_DIGIT_END) => mantissa = self.accumulate_digits(mantissa),
            _ => return Err(self.error(JsonSyntax::NoNumberAfterMinus)),
        }
        let exponent_mark = |unit: Option<u16>| unit.is_some_and(|unit| unit == b'e' as u16 || unit == b'E' as u16);
        let mut digits = self.index - integer_start;
        let fraction = self.peek() == Some(u16::from(b'.'));
        if !fraction && !exponent_mark(self.peek()) && digits <= JSON_EXACT_INTEGER_DIGITS {
            return Ok(match i32::try_from(mantissa) {
                Ok(magnitude) if !negative => JsonNumber::Integer(magnitude),
                Ok(magnitude) if magnitude != 0 => JsonNumber::Integer(-magnitude),
                _ => JsonNumber::Float(if negative { -(mantissa as f64) } else { mantissa as f64 }),
            });
        }
        let mut scale = 0_i32;
        if fraction {
            self.index += 1;
            if self.digit_value().is_none() {
                return Err(self.error(JsonSyntax::UnterminatedFraction));
            }
            let fraction_start = self.index;
            mantissa = self.accumulate_digits(mantissa);
            let fraction_digits = self.index - fraction_start;
            digits += fraction_digits;
            scale -= fraction_digits as i32;
        }
        if exponent_mark(self.peek()) {
            self.index += 1;
            let exponent_negative = self.take_if(b'-');
            if !exponent_negative {
                self.take_if(b'+');
            }
            if self.digit_value().is_none() {
                return Err(self.error(JsonSyntax::ExponentMissingNumber));
            }
            let mut exponent = 0_i32;
            while let Some(digit) = self.digit_value() {
                exponent = exponent.saturating_mul(10).saturating_add(digit as i32);
                self.index += 1;
            }
            scale = scale.saturating_add(if exponent_negative { -exponent } else { exponent });
        }
        let power = EXACT_POWERS_OF_TEN.get(scale.unsigned_abs() as usize);
        if digits <= JSON_MANTISSA_DIGITS
            && mantissa <= EXACT_DOUBLE_INTEGER
            && let Some(&power) = power
        {
            let magnitude = if scale < 0 { mantissa as f64 / power } else { mantissa as f64 * power };
            return Ok(JsonNumber::Float(if negative { -magnitude } else { magnitude }));
        }
        // The literal is ASCII by construction.
        let literal = &self.units[start..self.index];
        let mut inline = [0_u8; JSON_INLINE_NUMBER_LENGTH];
        let long;
        let bytes: &[u8] = if literal.len() <= JSON_INLINE_NUMBER_LENGTH {
            for (byte, unit) in inline.iter_mut().zip(literal) {
                *byte = unit.unit() as u8;
            }
            &inline[..literal.len()]
        } else {
            long = literal.iter().map(|unit| unit.unit() as u8).collect::<Vec<_>>();
            &long
        };
        let text = std::str::from_utf8(bytes).expect("a number literal is ASCII");
        Ok(JsonNumber::Float(text.parse::<f64>().expect("the grammar admits only decimal literals")))
    }

    /// `mantissa` followed by the decimal digits from the current unit, which the parse moves
    /// past; it wraps past `JSON_MANTISSA_DIGITS`, where its callers no longer read it.
    #[inline(always)]
    fn accumulate_digits(&mut self, mut mantissa: u64) -> u64 {
        let units = self.units;
        let mut at = self.index;
        while let Some(unit) = units.get(at)
            && let digit = unit.unit().wrapping_sub(JSON_DIGIT_START)
            && digit <= JSON_DIGIT_END - JSON_DIGIT_START
        {
            mantissa = mantissa.wrapping_mul(10).wrapping_add(u64::from(digit));
            at += 1;
        }
        self.index = at;
        mantissa
    }

    /// The value of the current unit when it is a decimal digit.
    #[inline(always)]
    fn digit_value(&self) -> Option<u64> {
        self.peek()
            .filter(|unit| (JSON_DIGIT_START..=JSON_DIGIT_END).contains(unit))
            .map(|unit| u64::from(unit - JSON_DIGIT_START))
    }

    /// The next unit past whitespace, which the parse moves past.
    #[inline(always)]
    fn token(&mut self) -> Option<u16> {
        match self.peek() {
            Some(unit) if is_json_whitespace(unit) => self.token_after_whitespace(),
            unit => unit,
        }
    }

    #[cold]
    fn token_after_whitespace(&mut self) -> Option<u16> {
        while self.peek().is_some_and(is_json_whitespace) {
            self.index += 1;
        }
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

impl<H: Host> Vm<H> {
    fn json_object(&mut self) -> Value {
        let prototype = self.realm_object_prototype(self.realm.globals);
        self.heap.alloc(Cell::Object(Self::empty_object(prototype)))
    }

    pub(super) fn json_raw_json(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let text = self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let wide = text.units().wide();
        let units: &[u16] = &wide;
        if units
            .first()
            .is_some_and(|unit| is_json_whitespace(*unit))
            || units
                .last()
                .is_some_and(|unit| is_json_whitespace(*unit))
        {
            return self.syntax_error_result(p, "Invalid raw JSON text");
        }
        let parsed = match JsonParser::new(units, &mut JsonTreeBuilder).parse() {
            Ok(parsed) => parsed,
            Err(error) => {
                return self.syntax_error_result(p, &json_syntax_message(units, error.syntax, error.position));
            }
        };
        if !matches!(
            match &parsed {
                JsonValue::Source(value, _) => value.as_ref(),
                value => value,
            },
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_)
        ) {
            return self.syntax_error_result(p, "rawJSON text must be a JSON primitive");
        }
        let object = self.json_object();
        let object_root = self.heap.root(object);
        if let Some(data) = self.object_data_mut(object) {
            data.set_proto(Value::NULL);
            data.set_raw_json();
        }
        let raw_atom = self.intern_atom("rawJSON");
        let raw_text_value = self.heap.alloc_string(text);
        let raw_text_root = self.heap.root(raw_text_value);
        let result = (|| {
            let key = self
                .heap
                .alloc_string(self.atom_name(raw_atom).into());
            let raw_text = self.heap.root_value(raw_text_root).unwrap();
            let descriptor = PropertyDescriptorRecord {
                writable: Some(false),
                configurable: Some(false),
                ..PropertyDescriptorRecord::data(raw_text)
            };
            let object = self.heap.root_value(object_root).unwrap();
            self.define_own_property_record(p, object, key, descriptor)?;
            let object = self.heap.root_value(object_root).unwrap();
            self.object_prevent_extensions(p, &[object])
        })();
        let object = self.heap.root_value(object_root).unwrap();
        self.heap.release_root(raw_text_root);
        self.heap.release_root(object_root);
        result.map(|_| object)
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
        let parsed = match parse_json_text(&text, &mut JsonTreeBuilder) {
            Ok(parsed) => parsed,
            Err(error) => return self.json_syntax_error(p, &text, error),
        };
        let value = self.parse_json_value(&parsed)?;
        let reviver_root = self.heap.root(reviver);
        let holder = self.json_object();
        let holder_root = self.heap.root(holder);
        let empty = self.intern_atom("");
        let result = (|| {
            self.set_property(holder, empty, value)?;
            let holder = self.heap.root_value(holder_root).unwrap();
            let key = JsString::from_str("");
            self.json_internalize(p, holder, &key, Some(&parsed), reviver_root)
        })();
        self.heap.release_root(holder_root);
        self.heap.release_root(reviver_root);
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

    /// Materializes a parsed tree with the same builder a reviver-free parse uses.
    fn parse_json_value(&mut self, value: &JsonValue) -> Result<Value, JsError> {
        let mut builder = JsonHeapBuilder::new(self);
        let value = builder.materialize(value)?;
        builder.error.map_or(Ok(value), Err)
    }

    fn json_source_text(&self, source: &JsonValue, current: Value) -> Option<JsString> {
        let JsonValue::Source(value, text) = source else {
            return None;
        };
        let unchanged = match value.as_ref() {
            JsonValue::Null => current.is_null(),
            JsonValue::Bool(expected) => current.as_bool() == Some(*expected),
            JsonValue::Number(expected) => self.same_value(current, Value::number(*expected)),
            JsonValue::String(expected) => {
                self.heap.string(current).is_some_and(|actual| actual == expected)
            }
            _ => false,
        };
        unchanged.then(|| text.clone())
    }

    fn json_internalize(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: &JsString,
        source: Option<&JsonValue>,
        reviver_root: crate::heap::RootId,
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        let holder_root = self.heap.root(holder);
        let mut value_root = None;
        let mut key_root = None;
        let mut context_root = None;
        let result = (|| {
            let key_atom = self.intern_js_atom(key);
            let holder = self.heap.root_value(holder_root).unwrap();
            let value = self.get_property(p, holder, key_atom)?;
            let value_handle = self.heap.root(value);
            value_root = Some(value_handle);
            let source_value = source.map(|source| match source {
                JsonValue::Source(value, _) => value.as_ref(),
                value => value,
            });
            let source_text = source.and_then(|source| self.json_source_text(source, value));
            if self.is_array(p, value)? {
                let array = self.heap.root_value(value_handle).unwrap();
                let length = self.array_like_length(p, array)?;
                let source_items = match source_value {
                    Some(JsonValue::Array(values)) => Some(values.as_slice()),
                    _ => None,
                };
                for index in 0..length {
                    let array = self.heap.root_value(value_handle).unwrap();
                    let child_key = JsString::from(index.to_string());
                    let child_source = source_items.and_then(|values| values.get(index));
                    let child =
                        self.json_internalize(p, array, &child_key, child_source, reviver_root)?;
                    self.json_update_property(p, value_handle, &child_key, child)?;
                }
            } else if self.is_object_like(value) {
                let object = self.heap.root_value(value_handle).unwrap();
                let keys = self.json_enumerable_keys(p, object)?;
                let source_properties = match source_value {
                    Some(JsonValue::Object(properties)) => Some(properties.as_slice()),
                    _ => None,
                };
                for child_key in keys {
                    let child_key = self.atom_value(child_key);
                    let object = self.heap.root_value(value_handle).unwrap();
                    let child_source = source_properties.and_then(|properties| {
                        properties
                            .iter()
                            .rev()
                            .find(|(name, _)| name == &child_key)
                            .map(|(_, value)| value)
                    });
                    let child =
                        self.json_internalize(p, object, &child_key, child_source, reviver_root)?;
                    self.json_update_property(p, value_handle, &child_key, child)?;
                }
            }
            let key_value = self.heap.alloc_string(key.clone());
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
        key: &JsString,
        value: Value,
    ) -> Result<(), JsError> {
        let value_root = self.heap.root(value);
        let key = self.heap.alloc_string(key.clone());
        let key_root = self.heap.root(key);
        let holder = self.heap.root_value(holder).unwrap();
        let key = self.heap.root_value(key_root).unwrap();
        let value = self.heap.root_value(value_root).unwrap();
        let result = if value.is_undefined() {
            self.object_delete_property(p, &[holder, key]).map(|_| ())
        } else {
            self.define_own_property_record(p, holder, key, PropertyDescriptorRecord::data(value))
                .map(|_| ())
        };
        self.heap.release_root(key_root);
        self.heap.release_root(value_root);
        result
    }

    pub(super) fn json_stringify(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let replacer = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let space = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        if replacer.is_undefined() && space.is_undefined() {
            let mut plain = JsonPlainWrite::new(self.intern_atom("toJSON"));
            plain.output = std::mem::take(&mut self.json_output);
            let written = self.json_write_plain(value, &mut plain);
            let text = written
                .filter(|&written| written)
                .map(|_| JsString::from_str(json_text_str(&plain.output)));
            plain.output.clear();
            if plain.output.capacity() <= JSON_RETAINED_OUTPUT_BYTES {
                self.json_output = plain.output;
            }
            match (written, text) {
                (Some(_), Some(text)) => return Ok(self.heap.alloc_string(text)),
                (Some(_), None) => return Ok(Value::UNDEFINED),
                (None, _) => {}
            }
        }
        let (replacer, property_list) = self.json_replacer(p, replacer)?;
        let space_root = self.heap.root(space);
        let gap = self.json_gap(p, space);
        self.heap.release_root(space_root);
        let gap = gap?;
        let mut state = JsonSerialization {
            replacer,
            property_list,
            gap,
            ancestors: Vec::new(),
            to_json: self.intern_atom("toJSON"),
            output: JsonText::new(),
            depth: 0,
            plans: Default::default(),
            steps: 0,
        };
        let holder = self.json_object();
        let holder_root = self.heap.root(holder);
        let empty = self.intern_atom("");
        self.set_property(holder, empty, value)?;
        let holder = self.heap.root_value(holder_root).unwrap_or(holder);
        let serialized = self.json_serialize_property(p, holder, JsonKey::Name(empty), &mut state);
        self.heap.release_root(holder_root);
        if !serialized? {
            return Ok(Value::UNDEFINED);
        }
        let text = json_text_string(state.output);
        Ok(self.heap.alloc_string(text.into()))
    }

    /// SerializeJSONProperty of `value` with no replacer and no gap, for plain data whose
    /// serialization runs no guest code: primitives, dense hole-free arrays and ordinary
    /// objects of non-dictionary shapes with only data members, none of which a `toJSON`
    /// reaches. `Some(false)` is an omitted value. `None` leaves the whole stringification to
    /// the general steps, having observed nothing: a cycle, getter, `toJSON`, function,
    /// BigInt, boxed primitive, index key, exotic object or nesting beyond
    /// `JSON_PLAIN_DEPTH`.
    fn json_write_plain(&self, value: Value, plain: &mut JsonPlainWrite) -> Option<bool> {
        if value.is_undefined() {
            return Some(false);
        }
        if value.is_null() {
            plain.output.extend_from_slice(b"null");
            return Some(true);
        }
        if let Some(value) = value.as_bool() {
            plain.output.extend_from_slice(if value { b"true" } else { b"false" });
            return Some(true);
        }
        if let Some(integer) = value.as_int() {
            write_json_integer(integer, &mut plain.output);
            return Some(true);
        }
        if let Some(number) = value.as_number() {
            if number.is_finite() {
                crate::number_to_string::write(number, &mut plain.output);
            } else {
                plain.output.extend_from_slice(b"null");
            }
            return Some(true);
        }
        if let Some(text) = self.heap.string(value) {
            write_json_string(text, &mut plain.output);
            return Some(true);
        }
        if matches!(self.heap.get(value), Some(Cell::Symbol(_))) {
            return Some(false);
        }
        if plain.ancestors.len() == JSON_PLAIN_DEPTH || plain.ancestors.contains(&value) {
            return None;
        }
        plain.ancestors.push(value);
        let written = if let Some(array) = self.heap.array(value) {
            self.json_write_plain_array(value, array, plain)
        } else {
            self.json_write_plain_object(value, plain)
        };
        plain.ancestors.pop();
        written.then_some(true)
    }

    fn json_write_plain_array(&self, value: Value, array: &crate::heap::ArrayObject, plain: &mut JsonPlainWrite) -> bool {
        if array.has_side_descriptors() {
            return false;
        }
        let key = (array.shape(), array.proto());
        if plain.array_lacks_to_json != Some(key) {
            if !self.json_lacks_to_json(value, plain.to_json) {
                return false;
            }
            plain.array_lacks_to_json = Some(key);
        }
        let Some(elements) = self.present_dense_elements(value, 0..array.elements().len()) else {
            return false;
        };
        plain.output.push(b'[');
        for (index, element) in elements.iter().enumerate() {
            if index != 0 {
                plain.output.push(b',');
            }
            match self.json_write_plain(*element, plain) {
                Some(true) => {}
                Some(false) => plain.output.extend_from_slice(b"null"),
                None => return false,
            }
        }
        plain.output.push(b']');
        true
    }

    fn json_write_plain_object(&self, value: Value, plain: &mut JsonPlainWrite) -> bool {
        let Some(data) = self.heap.object_record(value) else {
            return false;
        };
        // Shape facts decide the plan; these per-object facts do not.
        if data.is_raw_json() || data.has_side_descriptors() || self.boxed_primitive_data(value).is_some() {
            return false;
        }
        let Some(members) = self.json_plain_members(value, data, plain) else {
            return false;
        };
        plain.output.push(b'{');
        let mut written = false;
        for member in members.iter() {
            let Some(member_value) = self.heap.property_get(data, member.slot as usize) else {
                return false;
            };
            // The member is written before its value is known; an omitted value takes it back.
            let start = plain.output.len();
            if written {
                plain.output.push(b',');
            }
            let (key_start, key_end) = member.key;
            plain.output.extend_from_slice(&plain.key_text[key_start as usize..key_end as usize]);
            match self.json_write_plain(member_value, plain) {
                Some(true) => written = true,
                Some(false) => plain.output.truncate(start),
                None => return false,
            }
        }
        plain.output.push(b'}');
        true
    }

    /// The members of `value`'s (shape, prototype), derived on the first object of it: its
    /// enumerable string-keyed data properties in order, when it lacks `toJSON` and no
    /// property may be an accessor (whose kind a shape alone does not decide).
    fn json_plain_members(
        &self,
        value: Value,
        data: &crate::heap::Object,
        plain: &mut JsonPlainWrite,
    ) -> Option<std::rc::Rc<[JsonMember]>> {
        let key = (data.shape(), data.proto());
        if let Some(plan) = plain.object_plans.get(&key) {
            return plan.clone();
        }
        let plan = (!self.shape_may_have_accessors(data.shape())
            && self.json_lacks_to_json(value, plain.to_json))
        .then(|| self.shape_enumerable_slots(value))
        .flatten()
        .and_then(|slots| {
            slots
                .iter()
                .map(|&(atom, slot)| {
                    self.plain_named_key(atom).then(|| {
                        let start = plain.key_text.len() as u32;
                        self.write_json_key(atom, &mut plain.key_text);
                        plain.key_text.push(b':');
                        JsonMember { slot, key: (start, plain.key_text.len() as u32) }
                    })
                })
                .collect::<Option<std::rc::Rc<[JsonMember]>>>()
        });
        plain.object_plans.insert(key, plan.clone());
        plan
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
        let array_root = self.heap.root(value);
        let result = (|| {
            let array = self.heap.root_value(array_root).unwrap_or(value);
            let length = self.array_like_length(p, array)?;
            let mut names = Vec::new();
            for index in 0..length {
                let array = self.heap.root_value(array_root).unwrap_or(value);
                let item = self.get_index(p, array, Value::number(index as f64))?;
                let item_root = self.heap.root(item);
                let item = self.heap.root_value(item_root).unwrap_or(item);
                let accepted = item.as_number().is_some()
                    || self.heap.is_string(item)
                    || self.json_boxed_string_or_number(item).is_some();
                if accepted {
                    let name = match self.coerce_js_string(p, item) {
                        Ok(name) => name,
                        Err(error) => {
                            self.heap.release_root(item_root);
                            return Err(error);
                        }
                    };
                    let name = self.intern_js_atom(&name);
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
                self.heap.release_root(item_root);
            }
            Ok((None, Some(names)))
        })();
        self.heap.release_root(array_root);
        result
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
            let text = self.to_string(p, value)?;
            self.heap.alloc_string(text.into())
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

    fn json_serialize_property(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: JsonKey,
        state: &mut JsonSerialization,
    ) -> Result<bool, JsError> {
        let _stack = self.enter_stack()?;
        // The steps may run guest code, which can change the shapes plans describe.
        state.plans.clear();
        state.steps += 1;
        let holder_root = self.heap.root(holder);
        let result = (|| {
            let holder = self.heap.root_value(holder_root).unwrap_or(holder);
            let mut value = match key {
                JsonKey::Index(index) => self.get_index(p, holder, Value::number(index as f64))?,
                JsonKey::Name(atom) => self.get_property(p, holder, atom)?,
            };
            let mut value_root = self.heap.root(value);
            if self.is_object_like(value) || matches!(self.heap.get(value), Some(Cell::BigInt(_))) {
                let value_now = self.heap.root_value(value_root).unwrap_or(value);
                let method = match self.get_property(p, value_now, state.to_json) {
                    Ok(method) => method,
                    Err(error) => {
                        self.heap.release_root(value_root);
                        return Err(error);
                    }
                };
                if self.is_function(method) {
                    let method_root = self.heap.root(method);
                    let key_value = self.json_key_value(key);
                    let this = self.heap.root_value(value_root).unwrap_or(value);
                    let replacement = self.call_value(
                        p,
                        self.heap.root_value(method_root).unwrap_or(method),
                        this,
                        &[key_value],
                    );
                    self.heap.release_root(method_root);
                    self.heap.release_root(value_root);
                    value = replacement?;
                    value_root = self.heap.root(value);
                }
            }
            if let Some(replacer) = state.replacer {
                let replacer_root = self.heap.root(replacer);
                let key_value = self.json_key_value(key);
                let holder = self.heap.root_value(holder_root).unwrap_or(holder);
                let mut value = self.heap.root_value(value_root).unwrap_or(value);
                let replacement = self.call_value(
                    p,
                    self.heap.root_value(replacer_root).unwrap_or(replacer),
                    holder,
                    &[key_value, value],
                );
                self.heap.release_root(replacer_root);
                self.heap.release_root(value_root);
                value = replacement?;
                value_root = self.heap.root(value);
            }
            let value = self.heap.root_value(value_root).unwrap_or(value);
            let serialized = self.json_serialize_value(p, value, state);
            self.heap.release_root(value_root);
            serialized
        })();
        self.heap.release_root(holder_root);
        result
    }

    /// SerializeJSONProperty of a container's member, given the member's value when reading
    /// it runs no guest code (`plain_named_get`, a present dense element). With no replacer,
    /// a value that is no object or BigInt, or one that no `toJSON` reaches
    /// (`plain_property_lookup`), serializes as it is: the property steps would call nothing.
    /// Any other member takes them (`json_serialize_property`).
    fn json_member(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: JsonKey,
        value: Option<Value>,
        state: &mut JsonSerialization,
    ) -> Result<bool, JsError> {
        if state.replacer.is_none()
            && let Some(value) = value
        {
            let calls_nothing = if self.is_object_like(value) {
                self.json_object_plan(value, state).is_some()
                    || self.json_lacks_to_json(value, state.to_json)
            } else {
                !matches!(self.heap.get(value), Some(Cell::BigInt(_)))
            };
            if calls_nothing {
                // A nested container recurses here, as the property steps would.
                let _stack = self.enter_stack()?;
                return self.json_serialize_value(p, value, state);
            }
        }
        self.json_serialize_property(p, holder, key, state)
    }

    /// The member plan of `object` (`JsonObjectPlan`), derived once per shape and prototype
    /// while no guest code runs; `None` for any object the plan does not describe.
    fn json_object_plan(
        &self,
        object: Value,
        state: &mut JsonSerialization,
    ) -> Option<JsonObjectPlan> {
        let data = self.heap.object_record(object)?;
        let shape_key = (data.shape(), data.proto());
        if let Some(plan) = state.plans.get(&shape_key) {
            return plan.clone();
        }
        let plan = self.derive_json_object_plan(object, data, state.to_json);
        state.plans.insert(shape_key, plan.clone());
        plan
    }

    fn derive_json_object_plan(
        &self,
        object: Value,
        data: &Object,
        to_json: Atom,
    ) -> Option<JsonObjectPlan> {
        let shape = data.shape();
        if self.shape_is_dictionary(shape)
            || data.has_side_descriptors()
            || data.is_raw_json()
            || self.boxed_primitive_data(object).is_some()
            || !self.json_lacks_to_json(object, to_json)
        {
            return None;
        }
        let accessors = self.shape_may_have_accessors(shape);
        self.shape_enumerable_keys(object)?
            .into_iter()
            .map(|key| {
                let slot = self.property_shape_slot(shape, PropertyKey::string(key))?;
                let accessor = accessors && self.slot_attributes(data, slot)?.accessor;
                (self.plain_named_key(key) && !accessor).then(|| {
                    let mut quoted = Vec::new();
                    self.write_json_key(key, &mut quoted);
                    JsonPlanMember { key, slot, quoted: quoted.into_boxed_slice() }
                })
            })
            .collect()
    }

    /// Whether `Get(value, "toJSON")` finds nothing, decided without running guest code, for
    /// an object the serializer reads as it is, along a chain of arrays (such as
    /// `Array.prototype`) whose own shapes lack the key and of ordinary objects
    /// (`plain_property_lookup`). A boxed primitive, whose serialization coerces it, and any
    /// other object answer `false`.
    fn json_lacks_to_json(&self, value: Value, to_json: Atom) -> bool {
        let key = PropertyKey::string(to_json);
        if self.boxed_primitive_data(value).is_some() {
            return false;
        }
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

    /// A key's string value, allocated last before the call that receives it.
    fn json_key_value(&mut self, key: JsonKey) -> Value {
        let text = match key {
            JsonKey::Index(index) => JsString::from(index.to_string()),
            JsonKey::Name(atom) => self.atom_value(atom),
        };
        self.heap.alloc_string(text)
    }

    fn json_serialize_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        state: &mut JsonSerialization,
    ) -> Result<bool, JsError> {
        if value.is_undefined() || value.is_deleted() || self.is_function(value) {
            return Ok(false);
        }
        if value.is_null() {
            state.output.extend_from_slice(b"null");
            return Ok(true);
        }
        if let Some(value) = value.as_bool() {
            state.output.extend_from_slice(if value { b"true" } else { b"false" });
            return Ok(true);
        }
        if let Some(value) = value.as_number() {
            if value.is_finite() {
                crate::number_to_string::write(value, &mut state.output);
            } else {
                state.output.extend_from_slice(b"null");
            }
            return Ok(true);
        }
        let unboxed = self.json_unbox(p, value)?;
        if unboxed != value {
            return self.json_serialize_value(p, unboxed, state);
        }
        let value = unboxed;
        if let Some(raw) = self.json_raw_text(value) {
            state.output.extend_from_slice(raw.as_bytes());
            return Ok(true);
        }
        if self.heap.object_record(value).is_some() || self.heap.promise(value).is_some() {
            return self.json_serialize_object(p, value, state).map(|()| true);
        }
        if let Some(array) = self.heap.array(value) {
            // An arguments object is an ordinary object to IsArray, so it serializes as one.
            return if array.is_arguments_object() {
                self.json_serialize_object(p, value, state)
            } else {
                self.json_serialize_array(p, value, state)
            }
            .map(|()| true);
        }
        match self.heap.get(value).cloned() {
            _ if let Some(text) = self.heap.string(value).cloned()  => {
                write_json_string(&text, &mut state.output);
                Ok(true)
            }
            Some(Cell::Symbol(_)) => Ok(false),
            Some(Cell::BigInt(_)) => {
                Err(self.type_error(p, "Do not know how to serialize a BigInt".into()))
            }
            Some(Cell::Function(_)) | None => Ok(false),
            Some(Cell::Array { .. } | Cell::Promise(_)) => {
                unreachable!("heap.array and heap.promise classify their values before this match")
            }
            _ if self.is_array(p, value)? => {
                self.json_serialize_array(p, value, state).map(|()| true)
            }
            Some(Cell::Object(_)) => {
                self.json_serialize_object(p, value, state).map(|()| true)
            }
            Some(Cell::Proxy { .. })
            | Some(Cell::Date { .. })
            | Some(Cell::Error(_))
            | Some(Cell::Map { .. })
            | Some(Cell::Set { .. })
            | Some(Cell::ShadowRealm { .. })
            | Some(Cell::WeakMap { .. })
            | Some(Cell::WeakSet { .. })
            | Some(Cell::WeakRef { .. })
            | Some(Cell::FinalizationRegistry { .. })
            | Some(Cell::RegExp { .. })
            | Some(Cell::TypedArray { .. })
            | Some(Cell::ArrayBuffer { .. })
            | Some(Cell::DataView { .. })
            | Some(Cell::TemporalDuration { .. })
            | Some(Cell::TemporalPlainDate { .. })
            | Some(Cell::TemporalPlainDateTime { .. })
            | Some(Cell::TemporalPlainMonthDay { .. })
            | Some(Cell::TemporalPlainYearMonth { .. })
            | Some(Cell::TemporalZonedDateTime { .. })
            | Some(Cell::TemporalInstant { .. })
            | Some(Cell::WasmObject { .. })
            | Some(Cell::WasmGlobal { .. })
            | Some(Cell::WasmMemory { .. })
            | Some(Cell::WasmTable { .. }) => {
                self.json_serialize_object(p, value, state).map(|()| true)
            }
            Some(Cell::Environment { .. })
            | Some(Cell::Iterator(_))
            | Some(Cell::ArrayFromAsyncState(_))
            | Some(Cell::AwaitState(_))
            | Some(Cell::PromiseResolvingState { .. })
            | Some(Cell::WasmElements(_))
            | Some(Cell::WasmBits64(_))
            | Some(Cell::WasmGc { .. })
            | Some(Cell::WasmExtern(_))
            | Some(Cell::AccessorPair { .. })
            | Some(Cell::WasmTag { .. })
            | Some(Cell::WasmException { .. })
            | Some(Cell::WasmV128(_))
            | Some(Cell::BindingReference { .. })
            | Some(Cell::WasmHostFunction { .. }) => Ok(false),
        }
    }

    fn json_unbox(&mut self, p: &ResidualProgram, value: Value) -> Result<Value, JsError> {
        use crate::BuiltinBrand as B;
        match self.boxed_primitive_data(value) {
            Some((B::StringObject, _)) => {
                let text = self.to_string(p, value)?;
                Ok(self.heap.alloc_string(text.into()))
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

    fn json_serialize_array(
        &mut self,
        p: &ResidualProgram,
        array: Value,
        state: &mut JsonSerialization,
    ) -> Result<(), JsError> {
        self.json_push_ancestor(p, array, state)?;
        let array_root = self.heap.root(array);
        let result = (|| {
            let array = self.heap.root_value(array_root).unwrap_or(array);
            let length = self.array_like_length(p, array)?;
            state.output.push(b'[');
            state.depth += 1;
            for index in 0..length {
                let array = self.heap.root_value(array_root).unwrap_or(array);
                state.begin_member(index);
                let element = self
                    .present_dense_elements(array, index..index + 1)
                    .map(|element| element[0]);
                if !self.json_member(p, array, JsonKey::Index(index), element, state)? {
                    state.output.extend_from_slice(b"null");
                }
            }
            state.depth -= 1;
            state.end_container(length, b']');
            Ok(())
        })();
        self.heap.release_root(array_root);
        state.ancestors.pop();
        result
    }

    fn json_serialize_object(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        state: &mut JsonSerialization,
    ) -> Result<(), JsError> {
        self.json_push_ancestor(p, object, state)?;
        let object_root = self.heap.root(object);
        let result = (|| {
            let object = self.heap.root_value(object_root).unwrap_or(object);
            let plan = if state.property_list.is_none() && state.replacer.is_none() {
                self.json_object_plan(object, state)
            } else {
                None
            };
            let planned_steps = state.steps;
            let keys = match &plan {
                Some(_) => Vec::new(),
                None => match &state.property_list {
                    Some(property_list) => property_list.clone(),
                    None => self.json_enumerable_keys(p, object)?,
                },
            };
            let count = plan.as_ref().map_or(keys.len(), |plan| plan.len());
            state.output.push(b'{');
            state.depth += 1;
            let mut written = 0;
            for index in 0..count {
                let object = self.heap.root_value(object_root).unwrap_or(object);
                // The member is written before its value is known; an omitted value
                // takes it back.
                let member = state.output.len();
                state.begin_member(written);
                let key = match &plan {
                    Some(plan) => {
                        state.output.extend_from_slice(&plan[index].quoted);
                        plan[index].key
                    }
                    None => {
                        self.write_json_key(keys[index], &mut state.output);
                        keys[index]
                    }
                };
                state.output.extend_from_slice(if state.gap.is_empty() { b":" } else { b": " });
                // A plan read for this object holds while no member took the property steps.
                let planned = plan.as_ref().filter(|_| state.steps == planned_steps).and_then(|plan| {
                    self.heap
                        .ordinary_object(object)
                        .and_then(|data| self.heap.property_get(data, plan[index].slot))
                });
                let value = planned.or_else(|| self.plain_named_get(object, key));
                if self.json_member(p, object, JsonKey::Name(key), value, state)? {
                    written += 1;
                } else {
                    state.output.truncate(member);
                }
            }
            state.depth -= 1;
            state.end_container(written, b'}');
            Ok(())
        })();
        self.heap.release_root(object_root);
        state.ancestors.pop();
        result
    }

    /// QuoteJSONString of the property name `atom`, written from the atom's own text.
    /// `write_json_key` through the stringification's quoted keys (`JsonPlainWrite`).
    fn write_json_key(&self, atom: Atom, output: &mut JsonText) {
        match self.atom_text(atom) {
            AtomText::Host(text) => write_json_utf8(text.as_bytes(), output),
            AtomText::Js(text) => write_json_string(text, output),
        }
    }

    fn json_push_ancestor(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        state: &mut JsonSerialization,
    ) -> Result<(), JsError> {
        // Every ancestor and `value` is an object, whose SameValue is its identity.
        if state.ancestors.contains(&value) {
            return Err(self.type_error(p, "Converting circular structure to JSON".into()));
        }
        state.ancestors.push(value);
        Ok(())
    }

    fn json_enumerable_keys(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Vec<Atom>, JsError> {
        if let Some(keys) = self.shape_enumerable_keys(object) {
            return Ok(keys);
        }
        let object_root = self.heap.root(object);
        let mut keys_root = None;
        let result = (|| {
            let object = self.heap.root_value(object_root).unwrap();
            let keys = self.object_own_keys(p, object)?;
            let keys_handle = self.heap.root(keys);
            keys_root = Some(keys_handle);
            let length = self
                .heap
                .array_elements(keys)
                .map(|elements| elements.len())
                .expect("OwnPropertyKeys projects an array");
            let mut names = Vec::new();
            for index in 0..length {
                let keys = self.heap.root_value(keys_handle).unwrap();
                let key = self
                    .heap
                    .array_elements(keys)
                    .map(|elements| elements[index])
                    .expect("rooted own-key projection");
                if !self.heap.is_string(key) {
                    continue;
                }
                let key_root = self.heap.root(key);
                let current = (|| {
                    let key = self.heap.root_value(key_root).unwrap();
                    let object = self.heap.root_value(object_root).unwrap();
                    let descriptor = self.object_get_own_property_descriptor(p, &[object, key])?;
                    if self.descriptor_flag(descriptor, self.runtime_atoms.enumerable) {
                        let key = self.heap.root_value(key_root).unwrap();
                        if let Some(name) = self.heap.string(key).cloned() {
                            names.push(self.intern_js_atom(&name));
                        }
                    }
                    Ok(())
                })();
                self.heap.release_root(key_root);
                current?;
            }
            Ok(names)
        })();
        self.heap.release_root(object_root);
        if let Some(root) = keys_root {
            self.heap.release_root(root);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{JsString, JsonParser, JsonTreeBuilder, JsonUnit, JsonValue, WORD_BYTES, write_json_string};

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
        let JsonValue::Source(value, text) =
            JsonParser::new(&units, &mut JsonTreeBuilder).parse().unwrap()
        else {
            panic!("expected source-bearing primitive");
        };
        assert_eq!(text.units(), units);
        let JsonValue::String(value) = *value else {
            panic!("expected string");
        };
        assert_eq!(value.units(), &[0xD800, b'a' as u16, 0xDC00]);
    }
}
