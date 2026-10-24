use super::object_descriptors::PropertyDescriptorRecord;
use super::wtf16::{JsString, Units};
use super::*;
use crate::unicode;
use std::fmt::Write as _;

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

enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(JsString),
    Raw(String),
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
}

/// The key SerializeJSONProperty reads from its holder: an array index, or a property name.
/// Its string value is allocated only for the `toJSON` and replacer calls that receive it.
#[derive(Clone, Copy)]
enum JsonKey {
    Index(usize),
    Name(Atom),
}

fn write_json_string(value: &JsString, output: &mut String) {
    output.push('"');
    match value.units() {
        // ASCII runs that need no escape are copied whole.
        Units::Ascii(bytes) => {
            let mut run = 0;
            for (index, byte) in bytes.iter().enumerate() {
                let scalar = char::from(*byte);
                if json_escape_needed(scalar) {
                    output.push_str(ascii_text(&bytes[run..index]));
                    write_json_scalar(scalar, output);
                    run = index + 1;
                }
            }
            output.push_str(ascii_text(&bytes[run..]));
        }
        Units::Wide(units) => {
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
                    None => {
                        let _ = write!(output, "\\u{unit:04x}");
                    }
                }
            }
        }
    }
    output.push('"');
}

/// QuoteJSONString escapes every code point up to U+001F.
const LAST_CONTROL_CHARACTER: char = '\u{1f}';

fn ascii_text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("ASCII string units")
}

/// Whether QuoteJSONString writes `scalar` as an escape rather than as itself.
fn json_escape_needed(scalar: char) -> bool {
    matches!(scalar, '"' | '\\') || scalar <= LAST_CONTROL_CHARACTER
}

fn write_json_scalar(scalar: char, output: &mut String) {
    match scalar {
        '"' => output.push_str("\\\""),
        '\\' => output.push_str("\\\\"),
        '\u{08}' => output.push_str("\\b"),
        '\u{0c}' => output.push_str("\\f"),
        '\n' => output.push_str("\\n"),
        '\r' => output.push_str("\\r"),
        '\t' => output.push_str("\\t"),
        scalar if scalar <= LAST_CONTROL_CHARACTER => {
            let _ = write!(output, "\\u{:04x}", scalar as u32);
        }
        scalar => output.push(scalar),
    }
}

fn write_json(value: &JsonValue, output: &mut String, gap: &str, depth: usize) {
    match value {
        JsonValue::Null => output.push_str("null"),
        JsonValue::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        JsonValue::Number(value) => output.push_str(&crate::number_to_string::format(*value)),
        JsonValue::String(value) => write_json_string(value, output),
        JsonValue::Raw(value) => output.push_str(value),
        JsonValue::Source(value, _) => write_json(value, output, gap, depth),
        JsonValue::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push_str(if gap.is_empty() { "," } else { ",\n" });
                } else if !values.is_empty() && !gap.is_empty() {
                    output.push('\n');
                }
                write_indent(output, gap, depth + 1);
                write_json(value, output, gap, depth + 1);
            }
            if !values.is_empty() && !gap.is_empty() {
                output.push('\n');
                write_indent(output, gap, depth);
            }
            output.push(']');
        }
        JsonValue::Object(values) => {
            output.push('{');
            for (index, (key, value)) in values.iter().enumerate() {
                if index != 0 {
                    output.push_str(if gap.is_empty() { "," } else { ",\n" });
                } else if !values.is_empty() && !gap.is_empty() {
                    output.push('\n');
                }
                write_indent(output, gap, depth + 1);
                write_json_string(key, output);
                output.push_str(if gap.is_empty() { ":" } else { ": " });
                write_json(value, output, gap, depth + 1);
            }
            if !values.is_empty() && !gap.is_empty() {
                output.push('\n');
                write_indent(output, gap, depth);
            }
            output.push('}');
        }
    }
}

fn write_indent(output: &mut String, gap: &str, depth: usize) {
    for _ in 0..depth {
        output.push_str(gap);
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

/// What a parse makes of each JSON value: the source-bearing `JsonValue` tree a reviver
/// walks, or heap values directly when nothing reads the source afterwards.
trait JsonBuilder {
    type Value;
    type Key;
    fn literal(&mut self, value: JsonLiteral) -> Self::Value;
    fn number(&mut self, value: f64) -> Self::Value;
    fn string(&mut self, units: &[u16]) -> Self::Value;
    fn key(&mut self, units: &[u16]) -> Self::Key;
    fn array(&mut self, values: Vec<Self::Value>) -> Self::Value;
    fn object(&mut self, entries: Vec<(Self::Key, Self::Value)>) -> Self::Value;
    /// A primitive parsed from `source`, the text a reviver's context exposes.
    fn primitive(&mut self, value: Self::Value, source: &[u16]) -> Self::Value;
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
    fn string(&mut self, units: &[u16]) -> JsonValue {
        JsonValue::String(JsString::from_units(units))
    }
    fn key(&mut self, units: &[u16]) -> JsString {
        JsString::from_units(units)
    }
    fn array(&mut self, values: Vec<JsonValue>) -> JsonValue {
        JsonValue::Array(values)
    }
    fn object(&mut self, entries: Vec<(JsString, JsonValue)>) -> JsonValue {
        JsonValue::Object(entries)
    }
    fn primitive(&mut self, value: JsonValue, source: &[u16]) -> JsonValue {
        JsonValue::Source(Box::new(value), JsString::from_units(source))
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
}

enum JsonBuildError {
    Parse(JsonParseError),
    Runtime(JsError),
}

impl<'v, H: Host> JsonHeapBuilder<'v, H> {
    fn new(vm: &'v mut Vm<H>) -> Self {
        let globals = vm.realm.globals;
        Self {
            array_prototype: vm.array_prototype_for_realm(globals),
            object_prototype: vm.realm_object_prototype(globals),
            vm,
            error: None,
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
            JsonValue::String(value) => self.vm.heap.alloc(Cell::String(value.clone())),
            JsonValue::Raw(_) => unreachable!("raw JSON fragments are not parser values"),
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
    fn string(&mut self, units: &[u16]) -> Value {
        self.vm
            .heap
            .alloc(Cell::String(JsString::from_units(units)))
    }
    fn key(&mut self, units: &[u16]) -> Atom {
        self.vm.intern_unit_run_atom(Units::Wide(units))
    }
    fn array(&mut self, values: Vec<Value>) -> Value {
        self.vm.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Vm::<H>::empty_object(self.array_prototype),
                Rc::new(values),
            ),
        })
    }
    fn object(&mut self, entries: Vec<(Atom, Value)>) -> Value {
        let object = self
            .vm
            .heap
            .alloc(Cell::Object(Vm::<H>::empty_object(self.object_prototype)));
        for (atom, value) in entries {
            if let Err(error) = self.vm.set_property(object, atom, value) {
                self.error.get_or_insert(error);
            }
        }
        object
    }
    fn primitive(&mut self, value: Value, _source: &[u16]) -> Value {
        value
    }
}

struct JsonParser<'a> {
    units: &'a [u16],
    index: usize,
}

impl<'a> JsonParser<'a> {
    fn new(units: &'a [u16]) -> Self {
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
        let _stack =
            crate::stack::StackGuard::enter().map_err(|()| JsonParseError::StackExhausted)?;
        self.whitespace();
        let start = self.index;
        let primitive = match self.peek() {
            Some(JSON_NULL_INITIAL) => self.literal(b"null", JsonLiteral::Null, builder)?,
            Some(JSON_TRUE_INITIAL) => self.literal(b"true", JsonLiteral::True, builder)?,
            Some(JSON_FALSE_INITIAL) => self.literal(b"false", JsonLiteral::False, builder)?,
            Some(JSON_QUOTE) => {
                let units = self.string()?;
                builder.string(&units)
            }
            Some(JSON_ARRAY_START) => return self.array(builder),
            Some(JSON_OBJECT_START) => return self.object(builder),
            Some(JSON_MINUS | JSON_DIGIT_START..=JSON_DIGIT_END) => {
                let number = self.number()?;
                builder.number(number)
            }
            _ => return Err("expected JSON value".into()),
        };
        Ok(builder.primitive(primitive, &self.units[start..self.index]))
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

    /// A string's code units: borrowed from the source when it has no escapes.
    fn string(&mut self) -> Result<std::borrow::Cow<'a, [u16]>, JsonParseError> {
        self.expect(JSON_QUOTE as u8)?;
        let units = self.units;
        let start = self.index;
        let run = units[start..].iter().position(|unit| {
            *unit == JSON_QUOTE || *unit == JSON_ESCAPE || *unit < JSON_UNESCAPED_START
        });
        if let Some(length) = run
            && units[start + length] == JSON_QUOTE
        {
            self.index = start + length + 1;
            return Ok(std::borrow::Cow::Borrowed(&units[start..start + length]));
        }
        let mut output = Vec::new();
        loop {
            match self.take() {
                Some(JSON_QUOTE) => return Ok(std::borrow::Cow::Owned(output)),
                Some(JSON_ESCAPE) => self.escape(&mut output)?,
                Some(unit) if unit >= JSON_UNESCAPED_START => output.push(unit),
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

    fn array<B: JsonBuilder>(&mut self, builder: &mut B) -> Result<B::Value, JsonParseError> {
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
        self.expect(b'{')?;
        let mut entries = Vec::new();
        self.whitespace();
        if self.take_if(b'}') {
            return Ok(builder.object(entries));
        }
        loop {
            self.whitespace();
            let key = self.string()?;
            let key = builder.key(&key);
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
        self.take_if(b'-');
        match self.take() {
            Some(48) => {}
            Some(49..=57) => self.digits(),
            _ => return Err("invalid number".into()),
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
                *byte = *unit as u8;
            }
            &inline[..literal.len()]
        } else {
            long = literal.iter().map(|unit| *unit as u8).collect::<Vec<_>>();
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
        while self
            .peek()
            .is_some_and(|unit| JSON_WHITESPACE.contains(&unit))
        {
            self.index += 1;
        }
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
        self.units.get(self.index).copied()
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
            .is_some_and(|unit| JSON_WHITESPACE.contains(unit))
            || units
                .last()
                .is_some_and(|unit| JSON_WHITESPACE.contains(unit))
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
        let raw_text_value = self.heap.alloc(Cell::String(text));
        let raw_text_root = self.heap.root(raw_text_value);
        let result = (|| {
            let key = self
                .heap
                .alloc(Cell::String(self.atom_name(raw_atom).into()));
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
        let units = text.units().wide();
        if !self.is_function(reviver) {
            // Nothing revisits the parse, so values go straight to the heap.
            let mut builder = JsonHeapBuilder::new(self);
            let parsed = JsonParser::new(&units).parse(&mut builder);
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
        let parsed = match JsonParser::new(&units).parse(&mut JsonTreeBuilder) {
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
                matches!(self.heap.get(current), Some(Cell::String(actual)) if actual == expected)
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
            let key_value = self.heap.alloc(Cell::String(key.clone()));
            let key_handle = self.heap.root(key_value);
            key_root = Some(key_handle);
            let context = self.json_object();
            let context_handle = self.heap.root(context);
            context_root = Some(context_handle);
            if let Some(source) = source_text {
                let source_atom = self.intern_atom("source");
                let source_value = self.heap.alloc(Cell::String(source));
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
        let key = self.heap.alloc(Cell::String(key.clone()));
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
        };
        let holder = self.json_object();
        let holder_root = self.heap.root(holder);
        let empty = self.intern_atom("");
        self.set_property(holder, empty, value)?;
        let holder = self.heap.root_value(holder_root).unwrap_or(holder);
        let serialized = self.json_serialize_property(p, holder, JsonKey::Name(empty), &mut state);
        self.heap.release_root(holder_root);
        let Some(value) = serialized? else {
            return Ok(Value::UNDEFINED);
        };
        let mut text = String::new();
        write_json(&value, &mut text, &state.gap, 0);
        Ok(self.heap.alloc(Cell::String(text.into())))
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
                    || matches!(self.heap.get(item), Some(Cell::String(_)))
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
            self.heap.alloc(Cell::String(text.into()))
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
        if let Some(Cell::String(text)) = self.heap.get(value) {
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
    ) -> Result<Option<JsonValue>, JsError> {
        let _stack = self.enter_stack()?;
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

    /// A key's string value, allocated last before the call that receives it.
    fn json_key_value(&mut self, key: JsonKey) -> Value {
        let text = match key {
            JsonKey::Index(index) => JsString::from(index.to_string()),
            JsonKey::Name(atom) => self.atom_value(atom),
        };
        self.heap.alloc(Cell::String(text))
    }

    fn json_serialize_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        state: &mut JsonSerialization,
    ) -> Result<Option<JsonValue>, JsError> {
        if value.is_undefined() || value.is_deleted() || self.is_function(value) {
            return Ok(None);
        }
        if value.is_null() {
            return Ok(Some(JsonValue::Null));
        }
        if let Some(value) = value.as_bool() {
            return Ok(Some(JsonValue::Bool(value)));
        }
        if let Some(value) = value.as_number() {
            if !value.is_finite() {
                return Ok(Some(JsonValue::Null));
            }
            return Ok(Some(JsonValue::Number(value)));
        }
        let unboxed = self.json_unbox(p, value)?;
        if unboxed != value {
            return self.json_serialize_value(p, unboxed, state);
        }
        let value = unboxed;
        if let Some(raw) = self.json_raw_text(value) {
            return Ok(Some(JsonValue::Raw(raw)));
        }
        if self.heap.object_record(value).is_some() {
            return self.json_serialize_object(p, value, state).map(Some);
        }
        if self.heap.array(value).is_some() {
            return self.json_serialize_array(p, value, state).map(Some);
        }
        match self.heap.get(value).cloned() {
            Some(Cell::String(value)) => Ok(Some(JsonValue::String(value))),
            Some(Cell::Symbol(_)) => Ok(None),
            Some(Cell::BigInt(_)) => {
                Err(self.type_error(p, "Do not know how to serialize a BigInt".into()))
            }
            Some(Cell::Function { .. }) | None => Ok(None),
            Some(Cell::Array { .. }) => {
                unreachable!("heap.array classifies live arrays before this match")
            }
            _ if self.json_raw_text(value).is_some() => {
                Ok(self.json_raw_text(value).map(JsonValue::Raw))
            }
            _ if self.is_array(p, value)? => self.json_serialize_array(p, value, state).map(Some),
            Some(Cell::Object(_)) => self.json_serialize_object(p, value, state).map(Some),
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
            | Some(Cell::WasmTable { .. }) => self.json_serialize_object(p, value, state).map(Some),
            Some(Cell::Environment { .. })
            | Some(Cell::Iterator { .. })
            | Some(Cell::ArrayFromAsyncState(_))
            | Some(Cell::PromiseResolvingState { .. })
            | Some(Cell::WasmElements(_))
            | Some(Cell::WasmBits64(_))
            | Some(Cell::WasmGc { .. })
            | Some(Cell::WasmExtern(_))
            | Some(Cell::WasmTag { .. })
            | Some(Cell::WasmException { .. })
            | Some(Cell::WasmV128(_))
            | Some(Cell::BindingReference { .. })
            | Some(Cell::WasmHostFunction { .. }) => Ok(None),
        }
    }

    fn json_unbox(&mut self, p: &ResidualProgram, value: Value) -> Result<Value, JsError> {
        use crate::BuiltinBrand as B;
        match self.boxed_primitive_data(value) {
            Some((B::StringObject, _)) => {
                let text = self.to_string(p, value)?;
                Ok(self.heap.alloc(Cell::String(text.into())))
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
        match self.heap.get(self.own_property(value, raw)?) {
            Some(Cell::String(text)) => Some(text.host_string().to_owned()),
            _ => None,
        }
    }

    fn json_serialize_array(
        &mut self,
        p: &ResidualProgram,
        array: Value,
        state: &mut JsonSerialization,
    ) -> Result<JsonValue, JsError> {
        self.json_push_ancestor(p, array, state)?;
        let array_root = self.heap.root(array);
        let result = (|| {
            let array = self.heap.root_value(array_root).unwrap_or(array);
            let length = self.array_like_length(p, array)?;
            let mut output = Vec::with_capacity(length);
            for index in 0..length {
                let array = self.heap.root_value(array_root).unwrap_or(array);
                output.push(
                    self.json_serialize_property(p, array, JsonKey::Index(index), state)?
                        .unwrap_or(JsonValue::Null),
                );
            }
            Ok(JsonValue::Array(output))
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
    ) -> Result<JsonValue, JsError> {
        self.json_push_ancestor(p, object, state)?;
        let object_root = self.heap.root(object);
        let result = (|| {
            let object = self.heap.root_value(object_root).unwrap_or(object);
            let keys = if let Some(property_list) = &state.property_list {
                property_list.clone()
            } else {
                self.json_enumerable_keys(p, object)?
            };
            let mut output = Vec::new();
            for key in keys {
                let object = self.heap.root_value(object_root).unwrap_or(object);
                if let Some(value) =
                    self.json_serialize_property(p, object, JsonKey::Name(key), state)?
                {
                    output.push((self.atom_value(key), value));
                }
            }
            Ok(JsonValue::Object(output))
        })();
        self.heap.release_root(object_root);
        state.ancestors.pop();
        result
    }

    fn json_push_ancestor(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        state: &mut JsonSerialization,
    ) -> Result<(), JsError> {
        if state
            .ancestors
            .iter()
            .any(|ancestor| self.same_value(*ancestor, value))
        {
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
                if !matches!(self.heap.get(key), Some(Cell::String(_))) {
                    continue;
                }
                let key_root = self.heap.root(key);
                let current = (|| {
                    let key = self.heap.root_value(key_root).unwrap();
                    let object = self.heap.root_value(object_root).unwrap();
                    let descriptor = self.object_get_own_property_descriptor(p, &[object, key])?;
                    if self.descriptor_flag(descriptor, "enumerable") {
                        let key = self.heap.root_value(key_root).unwrap();
                        if let Some(Cell::String(name)) = self.heap.get(key).cloned() {
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
    use super::{JsString, JsonParser, JsonTreeBuilder, JsonValue, write_json_string};

    #[test]
    fn json_string_writer_preserves_lone_surrogates() {
        let value = JsString::from_units(&[0xD800, b'a' as u16, 0xDC00]);
        let mut output = String::new();
        write_json_string(&value, &mut output);
        assert_eq!(output, "\"\\ud800a\\udc00\"");
    }

    #[test]
    fn json_string_writer_keeps_surrogate_pairs_as_scalars() {
        let value = JsString::from_units(&[0xD83E, 0xDD80]);
        let mut output = String::new();
        write_json_string(&value, &mut output);
        assert_eq!(output, "\"🦀\"");
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
