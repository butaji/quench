use super::wtf16::JsString;
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
const JSON_RAW_VALUE_MARKER: &str = "\0rqj:raw-json";
const JSON_WHITESPACE: [u16; 4] = [b' ' as u16, b'\t' as u16, b'\n' as u16, b'\r' as u16];

enum JsonValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(JsString),
    Raw(String),
    Source(Box<JsonValue>, JsString),
    Array(Vec<JsonValue>),
    Object(Vec<(JsString, JsonValue)>),
}

struct JsonSerialization {
    replacer: Option<Value>,
    property_list: Option<Vec<JsString>>,
    gap: String,
    ancestors: Vec<Value>,
}

fn write_json_string(value: &JsString, output: &mut String) {
    output.push('"');
    let units = value.units();
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
        let Some(scalar) = scalar else {
            let _ = write!(output, "\\u{unit:04x}");
            continue;
        };
        match scalar {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            scalar if scalar <= '\u{1f}' => {
                let _ = write!(output, "\\u{:04x}", scalar as u32);
            }
            scalar => output.push(scalar),
        }
    }
    output.push('"');
}

fn write_json(value: &JsonValue, output: &mut String, gap: &str, depth: usize) {
    match value {
        JsonValue::Null => output.push_str("null"),
        JsonValue::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        JsonValue::Number(value) => output.push_str(&value.to_string()),
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

struct JsonParser<'a> {
    units: &'a [u16],
    index: usize,
}

impl<'a> JsonParser<'a> {
    fn new(units: &'a [u16]) -> Self {
        Self { units, index: 0 }
    }

    fn parse(mut self) -> Result<JsonValue, String> {
        let value = self.value()?;
        self.whitespace();
        (self.index == self.units.len())
            .then_some(value)
            .ok_or_else(|| "trailing characters".into())
    }

    fn value(&mut self) -> Result<JsonValue, String> {
        self.whitespace();
        let start = self.index;
        let value = match self.peek() {
            Some(JSON_NULL_INITIAL) => self.literal(b"null", JsonValue::Null),
            Some(JSON_TRUE_INITIAL) => self.literal(b"true", JsonValue::Bool(true)),
            Some(JSON_FALSE_INITIAL) => self.literal(b"false", JsonValue::Bool(false)),
            Some(JSON_QUOTE) => self.string().map(JsonValue::String),
            Some(JSON_ARRAY_START) => self.array(),
            Some(JSON_OBJECT_START) => self.object(),
            Some(JSON_MINUS | JSON_DIGIT_START..=JSON_DIGIT_END) => self.number(),
            _ => Err("expected JSON value".into()),
        }?;
        Ok(
            if matches!(
                &value,
                JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_)
            ) {
                JsonValue::Source(
                    Box::new(value),
                    JsString::from_units(&self.units[start..self.index]),
                )
            } else {
                value
            },
        )
    }

    fn literal(&mut self, expected: &[u8], value: JsonValue) -> Result<JsonValue, String> {
        if expected
            .iter()
            .copied()
            .all(|unit| self.take() == Some(u16::from(unit)))
        {
            Ok(value)
        } else {
            Err("invalid literal".into())
        }
    }

    fn string(&mut self) -> Result<JsString, String> {
        self.expect(JSON_QUOTE as u8)?;
        let mut units = Vec::new();
        loop {
            match self.take() {
                Some(JSON_QUOTE) => return Ok(JsString::from_units(&units)),
                Some(JSON_ESCAPE) => self.escape(&mut units)?,
                Some(unit) if unit >= JSON_UNESCAPED_START => units.push(unit),
                _ => return Err("unterminated JSON string".into()),
            }
        }
    }

    fn escape(&mut self, output: &mut Vec<u16>) -> Result<(), String> {
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

    fn hex_escape(&mut self) -> Result<u16, String> {
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

    fn array(&mut self) -> Result<JsonValue, String> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        self.whitespace();
        if self.take_if(b']') {
            return Ok(JsonValue::Array(values));
        }
        loop {
            values.push(self.value()?);
            self.whitespace();
            if self.take_if(b']') {
                return Ok(JsonValue::Array(values));
            }
            self.expect(b',')?;
        }
    }

    fn object(&mut self) -> Result<JsonValue, String> {
        self.expect(b'{')?;
        let mut values = Vec::new();
        self.whitespace();
        if self.take_if(b'}') {
            return Ok(JsonValue::Object(values));
        }
        loop {
            self.whitespace();
            let key = self.string()?;
            self.whitespace();
            self.expect(b':')?;
            values.push((key, self.value()?));
            self.whitespace();
            if self.take_if(b'}') {
                return Ok(JsonValue::Object(values));
            }
            self.expect(b',')?;
        }
    }

    fn number(&mut self) -> Result<JsonValue, String> {
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
        let text = String::from_utf16(&self.units[start..self.index])
            .map_err(|_| "invalid number".to_owned())?;
        let number = text
            .parse::<f64>()
            .map_err(|_| "invalid number".to_owned())?;
        let number =
            serde_json::Number::from_f64(number).ok_or_else(|| "invalid number".to_owned())?;
        Ok(JsonValue::Number(number))
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

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        (self.take() == Some(u16::from(expected)))
            .then_some(())
            .ok_or_else(|| format!("expected `{}`", expected as char))
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
    pub(super) fn json_raw_json(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let text = self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let units = text.units();
        if units
            .first()
            .is_some_and(|unit| JSON_WHITESPACE.contains(unit))
            || units
                .last()
                .is_some_and(|unit| JSON_WHITESPACE.contains(unit))
        {
            return self.syntax_error_result(p, "Invalid raw JSON text");
        }
        let parsed = match JsonParser::new(units).parse() {
            Ok(parsed) => parsed,
            Err(error) => return self.syntax_error_result(p, &error),
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
        let object = self.object();
        let object_root = self.heap.root(object);
        if let Some(data) = self.object_data_mut(object) {
            data.proto = Value::NULL;
        }
        let raw_atom = self.intern_atom("rawJSON");
        let marker_atom = self.intern_atom(JSON_RAW_VALUE_MARKER);
        let marker_value = Value::TRUE;
        let raw_text_value = self.heap.alloc(Cell::String(text));
        let raw_text_root = self.heap.root(raw_text_value);
        let result = (|| {
            let raw_text = self
                .heap
                .root_value(raw_text_root)
                .unwrap_or(raw_text_value);
            self.json_define_raw_property(p, object, raw_atom, raw_text)?;
            self.json_define_raw_property(p, object, marker_atom, marker_value)?;
            let object = self.heap.root_value(object_root).unwrap_or(object);
            self.object_prevent_extensions(p, &[object])
        })();
        let object = self.heap.root_value(object_root).unwrap_or(object);
        self.heap.release_root(raw_text_root);
        self.heap.release_root(object_root);
        result.map(|_| object)
    }

    pub(super) fn json_is_raw_json(&self, args: &[Value]) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let is_raw = matches!(self.heap.get(value), Some(Cell::Object(_)))
            && self
                .lookup_atom(JSON_RAW_VALUE_MARKER)
                .and_then(|atom| self.own_property(value, atom))
                .is_some_and(|marker| marker.as_bool() == Some(true));
        Ok(if is_raw { Value::TRUE } else { Value::FALSE })
    }

    fn json_define_raw_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        let descriptor = self.object();
        let descriptor_root = self.heap.root(descriptor);
        for (name, field) in [
            ("value", value),
            ("writable", Value::FALSE),
            ("enumerable", Value::FALSE),
            ("configurable", Value::FALSE),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, field)?;
        }
        let key = self.heap.alloc(Cell::String(self.atom_name(key).into()));
        let result = self.object_define_property(p, &[object, key, descriptor]);
        self.heap.release_root(descriptor_root);
        result.map(|_| ())
    }

    pub(super) fn json_parse(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let text = self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let parsed = match JsonParser::new(text.units()).parse() {
            Ok(parsed) => parsed,
            Err(error) => return self.syntax_error_result(p, &error),
        };
        let value = self.parse_json_value(&parsed)?;
        let reviver = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(reviver) {
            return Ok(value);
        }
        let reviver_root = self.heap.root(reviver);
        let holder = self.object();
        let holder_root = self.heap.root(holder);
        let empty = self.intern_atom("");
        self.set_property(holder, empty, value)?;
        let holder = self.heap.root_value(holder_root).unwrap_or(holder);
        let key = JsString::from_str("");
        let result = self.json_internalize(p, holder, &key, Some(&parsed), reviver_root);
        self.heap.release_root(holder_root);
        self.heap.release_root(reviver_root);
        result
    }

    fn parse_json_value(&mut self, value: &JsonValue) -> Result<Value, JsError> {
        Ok(match value {
            JsonValue::Null => Value::NULL,
            JsonValue::Bool(value) => {
                if *value {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            JsonValue::Number(value) => Value::number(value.as_f64().unwrap_or(f64::NAN)),
            JsonValue::String(value) => self.heap.alloc(Cell::String(value.clone())),
            JsonValue::Raw(_) => unreachable!("raw JSON fragments are not parser values"),
            JsonValue::Source(value, _) => self.parse_json_value(value)?,
            JsonValue::Array(values) => {
                let values = values
                    .iter()
                    .map(|value| self.parse_json_value(value))
                    .collect::<Result<Vec<_>, _>>()?;
                self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(values),
                })
            }
            JsonValue::Object(values) => {
                let object = self.object();
                for (key, value) in values {
                    let atom = self.intern_js_atom(key);
                    let value = self.parse_json_value(value)?;
                    self.set_property(object, atom, value)?;
                }
                object
            }
        })
    }

    fn json_source_text(&self, source: &JsonValue, current: Value) -> Option<JsString> {
        let JsonValue::Source(value, text) = source else {
            return None;
        };
        let unchanged = match value.as_ref() {
            JsonValue::Null => current.is_null(),
            JsonValue::Bool(expected) => current.as_bool() == Some(*expected),
            JsonValue::Number(expected) => current.as_number() == expected.as_f64(),
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
        let holder_root = self.heap.root(holder);
        let key_atom = self.intern_js_atom(key);
        let holder = self.heap.root_value(holder_root).unwrap_or(holder);
        let value = self.get_property(p, holder, key_atom)?;
        let value_root = self.heap.root(value);
        let source_value = source.map(|source| match source {
            JsonValue::Source(value, _) => value.as_ref(),
            value => value,
        });
        let source_text = source.and_then(|source| self.json_source_text(source, value));
        if self.is_array(p, value)? {
            let array = self.heap.root_value(value_root).unwrap_or(value);
            let length = self.array_like_length(p, array)?;
            let source_items = match source_value {
                Some(JsonValue::Array(values)) => Some(values.as_slice()),
                _ => None,
            };
            for index in 0..length {
                let array = self.heap.root_value(value_root).unwrap_or(value);
                let child_key = JsString::from(index.to_string());
                let child_source = source_items.and_then(|values| values.get(index));
                let child =
                    self.json_internalize(p, array, &child_key, child_source, reviver_root)?;
                let array = self.heap.root_value(value_root).unwrap_or(value);
                let child_key_value = self.heap.alloc(Cell::String(child_key.clone()));
                if child.is_undefined() {
                    self.object_delete_property(p, &[array, child_key_value])?;
                } else {
                    self.json_create_data_property(p, array, child_key_value, child)?;
                }
            }
        } else if self.is_object_like(value) {
            let object = self.heap.root_value(value_root).unwrap_or(value);
            let keys = self.json_enumerable_keys(p, object)?;
            let source_properties = match source_value {
                Some(JsonValue::Object(properties)) => Some(properties.as_slice()),
                _ => None,
            };
            for child_key in keys {
                let object = self.heap.root_value(value_root).unwrap_or(value);
                let child_source = source_properties.and_then(|properties| {
                    properties
                        .iter()
                        .rev()
                        .find(|(name, _)| name == &child_key)
                        .map(|(_, value)| value)
                });
                let child =
                    self.json_internalize(p, object, &child_key, child_source, reviver_root)?;
                let object = self.heap.root_value(value_root).unwrap_or(value);
                let key_value = self.heap.alloc(Cell::String(child_key.clone()));
                if child.is_undefined() {
                    self.object_delete_property(p, &[object, key_value])?;
                } else {
                    self.json_create_data_property(p, object, key_value, child)?;
                }
            }
        }
        let reviver = self
            .heap
            .root_value(reviver_root)
            .unwrap_or(Value::UNDEFINED);
        let holder = self.heap.root_value(holder_root).unwrap_or(holder);
        let key_value = self.heap.alloc(Cell::String(key.clone()));
        let key_root = self.heap.root(key_value);
        let mut value = self.heap.root_value(value_root).unwrap_or(value);
        let context = self.object();
        let context_root = self.heap.root(context);
        if let Some(source) = source_text {
            let source_atom = self.intern_atom("source");
            let source_value = self.heap.alloc(Cell::String(source));
            let source_root = self.heap.root(source_value);
            let context = self.heap.root_value(context_root).unwrap_or(context);
            let source_value = self.heap.root_value(source_root).unwrap_or(source_value);
            let set = self.set_property(context, source_atom, source_value);
            self.heap.release_root(source_root);
            set?;
        }
        let result = self.call_value(
            p,
            reviver,
            holder,
            &[
                self.heap.root_value(key_root).unwrap_or(key_value),
                value,
                self.heap.root_value(context_root).unwrap_or(context),
            ],
        );
        self.heap.release_root(context_root);
        self.heap.release_root(key_root);
        self.heap.release_root(value_root);
        self.heap.release_root(holder_root);
        value = result?;
        Ok(value)
    }

    fn json_create_data_property(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
        value: Value,
    ) -> Result<(), JsError> {
        if !matches!(self.heap.get(target), Some(Cell::Proxy { .. })) {
            let current = self.object_get_own_property_descriptor(p, &[target, key])?;
            if !current.is_undefined() && !self.descriptor_flag(current, "configurable") {
                return Ok(());
            }
        }
        let descriptor = self.object();
        let descriptor_root = self.heap.root(descriptor);
        for (name, field) in [
            ("value", value),
            ("writable", Value::TRUE),
            ("enumerable", Value::TRUE),
            ("configurable", Value::TRUE),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, field)?;
        }
        let result = self.object_define_property(p, &[target, key, descriptor]);
        self.heap.release_root(descriptor_root);
        result.map(|_| ())
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
        };
        let holder = self.object();
        let holder_root = self.heap.root(holder);
        let empty = self.intern_atom("");
        self.set_property(holder, empty, value)?;
        let holder = self.heap.root_value(holder_root).unwrap_or(holder);
        let root_key = JsString::from_str("");
        let serialized = self.json_serialize_property(p, holder, &root_key, &mut state);
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
    ) -> Result<(Option<Value>, Option<Vec<JsString>>), JsError> {
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
        for marker in ["\0rqj:string-value", "\0rqj:number-value"] {
            let Some(atom) = self.lookup_atom(marker) else {
                continue;
            };
            if let Some(value) = self.own_property(value, atom) {
                return Some(value);
            }
        }
        None
    }

    fn json_gap(&mut self, p: &ResidualProgram, value: Value) -> Result<String, JsError> {
        let number_box = self.json_boxed_value(value, "\0rqj:number-value").is_some();
        let string_box = self.json_boxed_value(value, "\0rqj:string-value").is_some();
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
                &text.units()[..text.units().len().min(10)],
            ));
        }
        Ok(String::new())
    }

    fn json_boxed_value(&self, value: Value, marker: &str) -> Option<Value> {
        self.lookup_atom(marker)
            .and_then(|atom| self.own_property(value, atom))
    }

    fn json_serialize_property(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: &JsString,
        state: &mut JsonSerialization,
    ) -> Result<Option<JsonValue>, JsError> {
        let holder_root = self.heap.root(holder);
        let key_value = self.heap.alloc(Cell::String(key.clone()));
        let key_root = self.heap.root(key_value);
        let result = (|| {
            let atom = self.intern_js_atom(key);
            let holder = self.heap.root_value(holder_root).unwrap_or(holder);
            let mut value = self.get_property(p, holder, atom)?;
            let mut value_root = self.heap.root(value);
            if self.is_object_like(value) || matches!(self.heap.get(value), Some(Cell::BigInt(_))) {
                let to_json = self.intern_atom("toJSON");
                let value_now = self.heap.root_value(value_root).unwrap_or(value);
                let method = match self.get_property(p, value_now, to_json) {
                    Ok(method) => method,
                    Err(error) => {
                        self.heap.release_root(value_root);
                        return Err(error);
                    }
                };
                if self.is_function(method) {
                    let method_root = self.heap.root(method);
                    let this = self.heap.root_value(value_root).unwrap_or(value);
                    let key_value = self.heap.root_value(key_root).unwrap_or(key_value);
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
                let holder = self.heap.root_value(holder_root).unwrap_or(holder);
                let key_value = self.heap.root_value(key_root).unwrap_or(key_value);
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
        self.heap.release_root(key_root);
        self.heap.release_root(holder_root);
        result
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
            let number =
                if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
                    serde_json::Number::from(value as i64)
                } else {
                    serde_json::Number::from_f64(value)
                        .unwrap_or_else(|| serde_json::Number::from(0))
                };
            return Ok(Some(JsonValue::Number(number)));
        }
        let unboxed = self.json_unbox(p, value)?;
        if unboxed != value {
            return self.json_serialize_value(p, unboxed, state);
        }
        let value = unboxed;
        match self.heap.get(value).cloned() {
            Some(Cell::String(value)) => Ok(Some(JsonValue::String(value))),
            Some(Cell::Symbol(_)) => Ok(None),
            Some(Cell::BigInt(_)) => {
                Err(self.type_error(p, "Do not know how to serialize a BigInt".into()))
            }
            Some(Cell::Function { .. }) | None => Ok(None),
            _ if self.json_raw_text(value).is_some() => {
                Ok(self.json_raw_text(value).map(JsonValue::Raw))
            }
            Some(Cell::Array { .. }) => self.json_serialize_array(p, value, state).map(Some),
            _ if self.is_array(p, value)? => self.json_serialize_array(p, value, state).map(Some),
            Some(Cell::Object(_))
            | Some(Cell::Proxy { .. })
            | Some(Cell::Date { .. })
            | Some(Cell::Error(_))
            | Some(Cell::Map { .. })
            | Some(Cell::Set { .. })
            | Some(Cell::WeakMap { .. })
            | Some(Cell::WeakSet { .. })
            | Some(Cell::WeakRef { .. })
            | Some(Cell::FinalizationRegistry { .. })
            | Some(Cell::RegExp { .. })
            | Some(Cell::TypedArray { .. })
            | Some(Cell::ArrayBuffer { .. })
            | Some(Cell::DataView { .. }) => self.json_serialize_object(p, value, state).map(Some),
            Some(Cell::Environment { .. })
            | Some(Cell::Iterator { .. })
            | Some(Cell::ArrayFromAsyncState(_)) => Ok(None),
        }
    }

    fn json_unbox(&mut self, p: &ResidualProgram, value: Value) -> Result<Value, JsError> {
        if self.json_boxed_value(value, "\0rqj:string-value").is_some() {
            let text = self.to_string(p, value)?;
            return Ok(self.heap.alloc(Cell::String(text.into())));
        }
        if self.json_boxed_value(value, "\0rqj:number-value").is_some() {
            return Ok(Value::number(self.to_number(p, value)?));
        }
        if let Some(value) = self.json_boxed_value(value, "\0rqj:boolean-value") {
            return Ok(value);
        }
        if let Some(value) = self.json_boxed_value(value, "\0rqj:bigint-value") {
            return Ok(value);
        }
        Ok(value)
    }

    fn json_raw_text(&self, value: Value) -> Option<String> {
        let marker = self.lookup_atom(JSON_RAW_VALUE_MARKER)?;
        if !self.own_property(value, marker)?.as_bool()? {
            return None;
        }
        let raw = self.lookup_atom("rawJSON")?;
        match self.heap.get(self.own_property(value, raw)?) {
            Some(Cell::String(text)) => Some(String::from_utf16_lossy(&text.units())),
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
                let key = JsString::from(index.to_string());
                output.push(
                    self.json_serialize_property(p, array, &key, state)?
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
                if let Some(value) = self.json_serialize_property(p, object, &key, state)? {
                    output.push((key, value));
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
    ) -> Result<Vec<JsString>, JsError> {
        let keys = self.object_own_keys(p, object)?;
        let keys_root = self.heap.root(keys);
        let values = match self.heap.get(keys) {
            Some(Cell::Array { elements, .. }) => elements.as_ref().clone(),
            _ => Vec::new(),
        };
        let object_root = self.heap.root(object);
        let result = (|| {
            let mut names = Vec::new();
            for key in values {
                if !matches!(self.heap.get(key), Some(Cell::String(_))) {
                    continue;
                }
                let key_root = self.heap.root(key);
                let key = self.heap.root_value(key_root).unwrap_or(key);
                let object = self.heap.root_value(object_root).unwrap_or(object);
                let descriptor = match self.object_get_own_property_descriptor(p, &[object, key]) {
                    Ok(descriptor) => descriptor,
                    Err(error) => {
                        self.heap.release_root(key_root);
                        return Err(error);
                    }
                };
                if self.descriptor_flag(descriptor, "enumerable") {
                    if let Some(Cell::String(name)) = self.heap.get(key) {
                        names.push(name.clone());
                    }
                }
                self.heap.release_root(key_root);
            }
            Ok(names)
        })();
        self.heap.release_root(object_root);
        self.heap.release_root(keys_root);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{JsString, JsonParser, JsonValue, write_json_string};

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
        let JsonValue::String(value) = JsonParser::new(&units).parse().unwrap() else {
            panic!("expected string");
        };
        assert_eq!(value.units(), &[0xD800, b'a' as u16, 0xDC00]);
    }

    #[test]
    fn json_parser_rejects_non_json_number_forms() {
        for source in ["01", "1+2", "1.", "1e"] {
            let units = source.encode_utf16().collect::<Vec<_>>();
            assert!(JsonParser::new(&units).parse().is_err(), "{source}");
        }
    }
}
