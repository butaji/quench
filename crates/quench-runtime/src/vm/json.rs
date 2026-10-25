use super::object_descriptors::PropertyDescriptorRecord;
use super::atom_keys::AtomText;
use super::wtf16::{JsString, Units};
use super::*;
use crate::unicode;
use std::io::Write as _;

const JSON_NULL_INITIAL: u16 = b'n' as u16;
const JSON_TRUE_INITIAL: u16 = b't' as u16;
const JSON_FALSE_INITIAL: u16 = b'f' as u16;
const JSON_QUOTE: u16 = b'"' as u16;
const JSON_ESCAPE: u16 = b'\\' as u16;
const JSON_ARRAY_START: u16 = b'[' as u16;
const JSON_OBJECT_START: u16 = b'{' as u16;
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

#[derive(Debug)]
enum JsonParseError {
    Syntax(String),
    StackExhausted,
}

impl From<String> for JsonParseError {
    fn from(message: String) -> Self {
        Self::Syntax(message)
    }
}

impl From<&str> for JsonParseError {
    fn from(message: &str) -> Self {
        Self::Syntax(message.into())
    }
}

/// Number literals at most this long parse from a stack buffer.
const JSON_INLINE_NUMBER_LENGTH: usize = 64;
/// Integer literals of at most this many digits are below 2^53, so accumulating their
/// digits gives the exact value the decimal conversion would.
const JSON_EXACT_INTEGER_DIGITS: usize = 15;

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
    /// A word of bytes at a time (`json_escape_bytes`, the bytes that end a run), then the
    /// rest one by one.
    #[inline(always)]
    fn string_run_length(units: &[u8]) -> Option<usize> {
        let mut words = units.chunks_exact(WORD_BYTES);
        let mut offset = 0;
        for word in &mut words {
            let word = u64::from_le_bytes(word.try_into().expect("a whole word"));
            let ends = json_escape_bytes(word);
            if ends != 0 {
                return Some(offset + (ends.trailing_zeros() / u8::BITS) as usize);
            }
            offset += WORD_BYTES;
        }
        words.remainder().iter().position(|unit| unit.ends_string_run()).map(|position| offset + position)
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
/// no widening, and decoded text whose units are not made parses from its UTF-8.
fn parse_json_text<B: JsonBuilder>(text: &JsString, builder: &mut B) -> Result<B::Value, JsonParseError> {
    if let Some(host) = text.unflattened_host() {
        return JsonParser::new(Utf8Byte::of(host)).parse(builder);
    }
    match text.units() {
        Units::Ascii(bytes) => JsonParser::new(bytes).parse(builder),
        Units::Wide(units) => JsonParser::new(units).parse(builder),
    }
}

/// What a parse makes of each JSON value: the source-bearing `JsonValue` tree a reviver
/// walks, or heap values directly when nothing reads the source afterwards.
trait JsonBuilder {
    type Value;
    type Key;
    fn literal(&mut self, value: JsonLiteral) -> Self::Value;
    fn number(&mut self, value: f64) -> Self::Value;
    fn string(&mut self, units: Units<'_>) -> Self::Value;
    fn key(&mut self, units: Units<'_>) -> Self::Key;
    fn array(&mut self, values: Vec<Self::Value>) -> Self::Value;
    fn object(&mut self, entries: Vec<(Self::Key, Self::Value)>) -> Self::Value;
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
    fn string(&mut self, units: Units<'_>) -> JsonValue {
        JsonValue::String(JsString::from_unit_run(units))
    }
    fn key(&mut self, units: Units<'_>) -> JsString {
        JsString::from_unit_run(units)
    }
    fn array(&mut self, values: Vec<JsonValue>) -> JsonValue {
        JsonValue::Array(values)
    }
    fn object(&mut self, entries: Vec<(JsString, JsonValue)>) -> JsonValue {
        JsonValue::Object(entries)
    }
    fn primitive(&mut self, value: JsonValue, source: Units<'_>) -> JsonValue {
        JsonValue::Source(Box::new(value), JsString::from_unit_run(source))
    }
}

/// Builds heap values as the parse goes. The heap does not collect inside a native call, so
/// values gathered for an array or object stay valid until their container holds them.
struct JsonHeapBuilder<'v, H: Host> {
    vm: &'v mut Vm<H>,
    array_prototype: Value,
    object_prototype: Value,
    /// The first property store that failed; the parse finishes and then reports it.
    error: Option<JsError>,
    /// The shape each (shape, key) add made, so objects of one layout, which a JSON text
    /// repeats, take their shapes without a transition search. Valid within one collection
    /// epoch, since a full collection may renumber shapes.
    transitions: rustc_hash::FxHashMap<(u32, Atom), u32>,
    transitions_epoch: u64,
    /// The shape an object starts from, whose `transitions` an object's keys follow.
    empty_shape: u32,
    /// The atom of each ASCII key the parse met: a JSON text repeats a few keys, which this
    /// hashes a word at a time instead of interning unit by unit (`intern_unit_run_atom`).
    /// Atoms outlive every collection.
    ascii_keys: rustc_hash::FxHashMap<Box<[u8]>, Atom>,
}

enum JsonBuildError {
    Parse(JsonParseError),
    Runtime(JsError),
}

impl<'v, H: Host> JsonHeapBuilder<'v, H> {
    fn new(vm: &'v mut Vm<H>) -> Self {
        let globals = vm.realm.globals;
        let object_prototype = vm.realm_object_prototype(globals);
        Self {
            array_prototype: vm.array_prototype_for_realm(globals),
            empty_shape: Vm::<H>::empty_object(object_prototype).shape(),
            object_prototype,
            transitions_epoch: vm.heap.collection_epoch(),
            vm,
            error: None,
            transitions: Default::default(),
            ascii_keys: Default::default(),
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
                let values = values
                    .iter()
                    .map(|value| self.materialize(value))
                    .collect::<Result<Vec<_>, _>>()?;
                self.array(values)
            }
            JsonValue::Object(entries) => {
                let entries = entries
                    .iter()
                    .map(|(key, value)| {
                        let key = self.vm.intern_js_atom(key);
                        Ok((key, self.materialize(value)?))
                    })
                    .collect::<Result<Vec<_>, JsError>>()?;
                self.object(entries)
            }
        })
    }
}

impl<H: Host> JsonBuilder for JsonHeapBuilder<'_, H> {
    type Value = Value;
    type Key = Atom;
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
    fn string(&mut self, units: Units<'_>) -> Value {
        self.vm.heap.alloc_string(JsString::from_unit_run(units))
    }
    fn key(&mut self, units: Units<'_>) -> Atom {
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
    fn array(&mut self, values: Vec<Value>) -> Value {
        self.vm.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Vm::<H>::empty_object(self.array_prototype),
                values,
            ),
        })
    }
    fn object(&mut self, entries: Vec<(Atom, Value)>) -> Value {
        let epoch = self.vm.heap.collection_epoch();
        if epoch != self.transitions_epoch {
            self.transitions.clear();
            self.transitions_epoch = epoch;
        }
        // Keys whose adds this parse made before end at a known shape, each a one-slot
        // append (`shape_appends_one_slot`): the object is made at once with its values.
        let shape = entries.iter().try_fold(self.empty_shape, |shape, (atom, _)| {
            self.transitions.get(&(shape, *atom)).copied()
        });
        if let Some(shape) = shape {
            let values = entries.iter().map(|(_, value)| *value).collect::<smallvec::SmallVec<[Value; 16]>>();
            return self.vm.heap.alloc_object_with_properties(self.object_prototype, shape, &values);
        }
        let object = self
            .vm
            .heap
            .alloc(Cell::Object(Vm::<H>::empty_object(self.object_prototype)));
        let shape_of = |vm: &Vm<H>| vm.object_data(object).map(|data| data.shape());
        let mut shape = shape_of(self.vm);
        for (atom, value) in entries {
            let epoch = self.vm.heap.collection_epoch();
            if epoch != self.transitions_epoch {
                self.transitions.clear();
                self.transitions_epoch = epoch;
            }
            if let Some(from) = shape
                && let Some(&to) = self.transitions.get(&(from, atom))
            {
                self.vm.heap.property_add(object, value, to);
                shape = Some(to);
                continue;
            }
            if let Err(error) = self.vm.set_property(object, atom, value) {
                self.error.get_or_insert(error);
            }
            let after = shape_of(self.vm);
            if let (Some(from), Some(to)) = (shape, after)
                && self.vm.heap.collection_epoch() == self.transitions_epoch
                && self.vm.shape_appends_one_slot(from, to)
            {
                self.transitions.insert((from, atom), to);
            }
            shape = after;
        }
        object
    }
    fn primitive(&mut self, value: Value, _source: Units<'_>) -> Value {
        value
    }
}

struct JsonParser<'a, T> {
    units: &'a [T],
    index: usize,
}

impl<'a, T: JsonUnit> JsonParser<'a, T> {
    fn new(units: &'a [T]) -> Self {
        Self { units, index: 0 }
    }

    fn parse<B: JsonBuilder>(mut self, builder: &mut B) -> Result<B::Value, JsonParseError> {
        let value = self.value(builder)?;
        self.whitespace();
        (self.index == self.units.len())
            .then_some(value)
            .ok_or_else(|| "trailing characters".into())
    }

    fn value<B: JsonBuilder>(&mut self, builder: &mut B) -> Result<B::Value, JsonParseError> {
        self.whitespace();
        let start = self.index;
        let primitive = match self.peek() {
            Some(JSON_NULL_INITIAL) => self.literal(b"null", JsonLiteral::Null, builder)?,
            Some(JSON_TRUE_INITIAL) => self.literal(b"true", JsonLiteral::True, builder)?,
            Some(JSON_FALSE_INITIAL) => self.literal(b"false", JsonLiteral::False, builder)?,
            Some(JSON_QUOTE) => {
                let text = self.string()?;
                builder.string(text.units())
            }
            Some(JSON_ARRAY_START) => return self.array(builder),
            Some(JSON_OBJECT_START) => return self.object(builder),
            Some(JSON_MINUS | JSON_DIGIT_START..=JSON_DIGIT_END) => {
                let number = self.number()?;
                builder.number(number)
            }
            _ => return Err("expected JSON value".into()),
        };
        Ok(builder.primitive(primitive, T::run(&self.units[start..self.index]).units()))
    }

    fn literal<B: JsonBuilder>(
        &mut self,
        expected: &[u8],
        value: JsonLiteral,
        builder: &mut B,
    ) -> Result<B::Value, JsonParseError> {
        if expected
            .iter()
            .copied()
            .all(|unit| self.take() == Some(u16::from(unit)))
        {
            Ok(builder.literal(value))
        } else {
            Err("invalid literal".into())
        }
    }

    /// A string's code units: a run of the source when it has no escapes.
    fn string(&mut self) -> Result<JsonStringUnits<'a>, JsonParseError> {
        self.expect(JSON_QUOTE as u8)?;
        let units = self.units;
        let start = self.index;
        let run = T::string_run_length(&units[start..]);
        if let Some(length) = run
            && units[start + length].unit() == JSON_QUOTE
        {
            self.index = start + length + 1;
            return Ok(T::run(&units[start..start + length]));
        }
        let mut output = Vec::new();
        loop {
            let run = &units[self.index..];
            let length = T::string_run_length(run).unwrap_or(run.len());
            T::extend_units(&run[..length], &mut output);
            self.index += length;
            match self.take() {
                Some(JSON_QUOTE) => return Ok(JsonStringUnits::Made(output)),
                Some(JSON_ESCAPE) => self.escape(&mut output)?,
                _ => return Err("unterminated JSON string".into()),
            }
        }
    }

    fn escape(&mut self, output: &mut Vec<u16>) -> Result<(), JsonParseError> {
        let unit = self
            .take()
            .ok_or_else(|| "unterminated escape".to_owned())?;
        match unit {
            value if value == JSON_QUOTE || value == JSON_ESCAPE || value == b'/' as u16 => {
                output.push(value)
            }
            value if value == b'b' as u16 => output.push(JSON_BACKSPACE),
            value if value == b'f' as u16 => output.push(JSON_FORM_FEED),
            value if value == b'n' as u16 => output.push(b'\n' as u16),
            value if value == b'r' as u16 => output.push(b'\r' as u16),
            value if value == b't' as u16 => output.push(b'\t' as u16),
            value if value == b'u' as u16 => output.push(self.hex_escape()?),
            _ => return Err("invalid JSON escape".into()),
        }
        Ok(())
    }

    fn hex_escape(&mut self) -> Result<u16, JsonParseError> {
        let mut value = 0;
        for _ in 0..JSON_HEX_ESCAPE_DIGITS {
            let digit = self
                .take()
                .and_then(hex_digit)
                .ok_or_else(|| "invalid Unicode escape".to_owned())?;
            value = value * 16 + digit;
        }
        Ok(value)
    }

    /// A nested value's parse recurses only through `array` and `object`, which therefore
    /// hold the stack guard.
    fn nesting_guard() -> Result<crate::stack::StackGuard, JsonParseError> {
        crate::stack::StackGuard::enter().map_err(|()| JsonParseError::StackExhausted)
    }

    fn array<B: JsonBuilder>(&mut self, builder: &mut B) -> Result<B::Value, JsonParseError> {
        let _stack = Self::nesting_guard()?;
        self.expect(b'[')?;
        let mut values = Vec::new();
        self.whitespace();
        if self.take_if(b']') {
            return Ok(builder.array(values));
        }
        loop {
            values.push(self.value(builder)?);
            self.whitespace();
            if self.take_if(b']') {
                return Ok(builder.array(values));
            }
            self.expect(b',')?;
        }
    }

    fn object<B: JsonBuilder>(&mut self, builder: &mut B) -> Result<B::Value, JsonParseError> {
        let _stack = Self::nesting_guard()?;
        self.expect(b'{')?;
        let mut entries = Vec::new();
        self.whitespace();
        if self.take_if(b'}') {
            return Ok(builder.object(entries));
        }
        loop {
            self.whitespace();
            let key = self.string()?;
            let key = builder.key(key.units());
            self.whitespace();
            self.expect(b':')?;
            entries.push((key, self.value(builder)?));
            self.whitespace();
            if self.take_if(b'}') {
                return Ok(builder.object(entries));
            }
            self.expect(b',')?;
        }
    }

    fn number(&mut self) -> Result<f64, JsonParseError> {
        let start = self.index;
        let negative = self.take_if(b'-');
        let integer_start = self.index;
        match self.take() {
            Some(48) => {}
            Some(49..=57) => self.digits(),
            _ => return Err("invalid number".into()),
        }
        let integer = &self.units[integer_start..self.index];
        if integer.len() <= JSON_EXACT_INTEGER_DIGITS
            && !self.peek().is_some_and(|unit| unit == u16::from(b'.') || unit == 69 || unit == 101)
        {
            let magnitude = integer
                .iter()
                .fold(0_u64, |value, digit| value * 10 + u64::from(digit.unit() - JSON_DIGIT_START));
            let magnitude = magnitude as f64;
            return Ok(if negative { -magnitude } else { magnitude });
        }
        if self.take_if(b'.') {
            if !self.digit() {
                return Err("invalid number".into());
            }
            self.digits();
        }
        if self.peek().is_some_and(|unit| unit == 69 || unit == 101) {
            self.index += 1;
            if !self.take_if(b'+') {
                self.take_if(b'-');
            }
            if !self.digit() {
                return Err("invalid number".into());
            }
            self.digits();
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
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .ok_or_else(|| "invalid number".into())
    }

    fn digits(&mut self) {
        while self.digit() {}
    }

    fn digit(&mut self) -> bool {
        if self
            .peek()
            .is_some_and(|unit| (JSON_DIGIT_START..=JSON_DIGIT_END).contains(&unit))
        {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn whitespace(&mut self) {
        let rest = &self.units[self.index..];
        self.index += rest.iter().take_while(|unit| is_json_whitespace(unit.unit())).count();
    }

    fn expect(&mut self, expected: u8) -> Result<(), JsonParseError> {
        (self.take() == Some(u16::from(expected)))
            .then_some(())
            .ok_or_else(|| JsonParseError::Syntax(format!("expected `{}`", expected as char)))
    }

    fn take_if(&mut self, expected: u8) -> bool {
        if self.peek() == Some(u16::from(expected)) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn take(&mut self) -> Option<u16> {
        let value = self.peek()?;
        self.index += 1;
        Some(value)
    }

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
        let parsed = match JsonParser::new(units).parse(&mut JsonTreeBuilder) {
            Ok(parsed) => parsed,
            Err(JsonParseError::Syntax(error)) => return self.syntax_error_result(p, &error),
            Err(JsonParseError::StackExhausted) => return Err(self.stack_exhaustion_error()),
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
                Err(JsonBuildError::Parse(JsonParseError::Syntax(error))) => {
                    self.syntax_error_result(p, &error)
                }
                Err(JsonBuildError::Parse(JsonParseError::StackExhausted)) => {
                    Err(self.stack_exhaustion_error())
                }
                Err(JsonBuildError::Runtime(error)) => Err(error),
            };
        }
        let parsed = match parse_json_text(&text, &mut JsonTreeBuilder) {
            Ok(parsed) => parsed,
            Err(JsonParseError::Syntax(error)) => return self.syntax_error_result(p, &error),
            Err(JsonParseError::StackExhausted) => return Err(self.stack_exhaustion_error()),
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
            if let Some(written) = self.json_write_plain(value, &mut plain) {
                if !written {
                    return Ok(Value::UNDEFINED);
                }
                let text = json_text_string(plain.output);
                return Ok(self.heap.alloc_string(text.into()));
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
        self.boxed_primitive_data(value).and_then(|(brand, payload)| {
            matches!(brand, crate::BuiltinBrand::StringObject | crate::BuiltinBrand::NumberObject).then_some(payload)
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
        // Every byte value at every position of a word and past it, after every prefix byte
        // class, as the per-unit definition finds it.
        let fill = [b'a', 0x7f, 0x80, 0xff, b' ', b'!'];
        for &prefix in &fill {
            for length in 0..(2 * WORD_BYTES + 3) {
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
            JsonParser::new(&units).parse(&mut JsonTreeBuilder).unwrap()
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
