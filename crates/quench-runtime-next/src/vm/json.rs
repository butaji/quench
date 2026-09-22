use super::wtf16::JsString;
use super::*;
use std::fmt::Write as _;

enum JsonValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(JsString),
    Array(Vec<JsonValue>),
    Object(Vec<(JsString, JsonValue)>),
}

fn write_json_string(value: &JsString, output: &mut String) {
    output.push('"');
    let units = value.units();
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        let scalar = if (0xD800..=0xDBFF).contains(&unit)
            && units
                .get(index + 1)
                .is_some_and(|next| (0xDC00..=0xDFFF).contains(next))
        {
            let high = u32::from(unit) - 0xD800;
            let low = u32::from(units[index + 1]) - 0xDC00;
            index += 2;
            Some(char::from_u32(0x1_0000 + (high << 10) + low).expect("valid surrogate pair"))
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

fn write_json(value: &JsonValue, output: &mut String) {
    match value {
        JsonValue::Null => output.push_str("null"),
        JsonValue::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        JsonValue::Number(value) => output.push_str(&value.to_string()),
        JsonValue::String(value) => write_json_string(value, output),
        JsonValue::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_json(value, output);
            }
            output.push(']');
        }
        JsonValue::Object(values) => {
            output.push('{');
            for (index, (key, value)) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_json_string(key, output);
                output.push(':');
                write_json(value, output);
            }
            output.push('}');
        }
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
        match self.peek() {
            Some(110) => self.literal(b"null", JsonValue::Null),
            Some(116) => self.literal(b"true", JsonValue::Bool(true)),
            Some(102) => self.literal(b"false", JsonValue::Bool(false)),
            Some(34) => self.string().map(JsonValue::String),
            Some(91) => self.array(),
            Some(123) => self.object(),
            Some(45 | 48..=57) => self.number(),
            _ => Err("expected JSON value".into()),
        }
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
        self.expect(b'"')?;
        let mut units = Vec::new();
        loop {
            match self.take() {
                Some(34) => return Ok(JsString::from_units(&units)),
                Some(92) => self.escape(&mut units)?,
                Some(unit) if unit >= 0x20 => units.push(unit),
                _ => return Err("unterminated JSON string".into()),
            }
        }
    }

    fn escape(&mut self, output: &mut Vec<u16>) -> Result<(), String> {
        let unit = self
            .take()
            .ok_or_else(|| "unterminated escape".to_owned())?;
        match unit {
            34 | 92 | 47 => output.push(unit),
            98 => output.push(0x08),
            102 => output.push(0x0c),
            110 => output.push(b'\n' as u16),
            114 => output.push(b'\r' as u16),
            116 => output.push(b'\t' as u16),
            117 => output.push(self.hex_escape()?),
            _ => return Err("invalid JSON escape".into()),
        }
        Ok(())
    }

    fn hex_escape(&mut self) -> Result<u16, String> {
        let mut value = 0;
        for _ in 0..4 {
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
        if self.take_if(b'.') && !self.digit() {
            return Err("invalid number".into());
        }
        if self.peek().is_some_and(|unit| unit == 69 || unit == 101) {
            self.index += 1;
            self.take_if(b'+');
            self.take_if(b'-');
            if !self.digit() {
                return Err("invalid number".into());
            }
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
        if self.peek().is_some_and(|unit| (48..=57).contains(&unit)) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn whitespace(&mut self) {
        while self
            .peek()
            .is_some_and(|unit| matches!(unit, 0x20 | 0x09 | 0x0a | 0x0d))
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
        48..=57 => Some(unit - 48),
        97..=102 => Some(unit - 97 + 10),
        65..=70 => Some(unit - 65 + 10),
        _ => None,
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn json_parse(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let text = self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let parsed = JsonParser::new(text.units())
            .parse()
            .map_err(|error| JsError(format!("JSON parse: {error}").into()))?;
        self.parse_json_value(&parsed)
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

    pub(super) fn json_stringify(
        &mut self,
        _p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let Some(value) = self.to_json(value, false, &mut Vec::new())? else {
            return Ok(Value::UNDEFINED);
        };
        let mut text = String::new();
        write_json(&value, &mut text);
        Ok(self.heap.alloc(Cell::String(text.into())))
    }

    #[expect(clippy::wrong_self_convention)]
    fn to_json(
        &mut self,
        value: Value,
        array_element: bool,
        ancestors: &mut Vec<Value>,
    ) -> Result<Option<JsonValue>, JsError> {
        if value.is_undefined() || value.is_deleted() {
            return Ok(array_element.then_some(JsonValue::Null));
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
        match self.heap.get(value).cloned() {
            Some(Cell::String(value)) => Ok(Some(JsonValue::String(value))),
            Some(Cell::BigInt(_)) | Some(Cell::Symbol(_)) => {
                Err(JsError("JSON cannot stringify this value".into()))
            }
            Some(Cell::Function { .. }) => Ok(None),
            Some(Cell::Array { elements, .. }) => {
                if ancestors.contains(&value) {
                    return Err(JsError(
                        "JSON.stringify cannot serialize cyclic structures".into(),
                    ));
                }
                ancestors.push(value);
                let mut output = Vec::with_capacity(elements.len());
                let result = (|| {
                    for value in elements.iter().copied() {
                        output.push(
                            self.to_json(value, true, ancestors)?
                                .unwrap_or(JsonValue::Null),
                        );
                    }
                    Ok(Some(JsonValue::Array(output)))
                })();
                ancestors.pop();
                result
            }
            Some(Cell::Map { .. })
            | Some(Cell::ArrayBuffer { .. })
            | Some(Cell::TypedArray { .. })
            | Some(Cell::DataView { .. })
            | Some(Cell::Set { .. })
            | Some(Cell::WeakMap { .. })
            | Some(Cell::WeakSet { .. })
            | Some(Cell::WeakRef { .. })
            | Some(Cell::FinalizationRegistry { .. }) => Ok(Some(JsonValue::Object(Vec::new()))),
            Some(Cell::Object(object)) => {
                if ancestors.contains(&value) {
                    return Err(JsError(
                        "JSON.stringify cannot serialize cyclic structures".into(),
                    ));
                }
                ancestors.push(value);
                let shape = object.shape();
                let keys = self.shapes[shape as usize].keys.clone();
                let mut output = Vec::new();
                let result = (|| {
                    for (slot, atom) in keys.into_iter().enumerate() {
                        let Some(value) = self.heap.property_get(&object, slot) else {
                            continue;
                        };
                        if let Some(value) = self.to_json(value, false, ancestors)? {
                            output.push((self.atom_value(atom), value));
                        }
                    }
                    Ok(Some(JsonValue::Object(output)))
                })();
                ancestors.pop();
                result
            }
            Some(Cell::Date {
                milliseconds: value,
                ..
            }) => Ok(self
                .date_to_json_string(value)
                .map(|value| JsonValue::String(value.into()))
                .or(Some(JsonValue::Null))),
            Some(Cell::Error(value)) => Ok(Some(JsonValue::String(value.into()))),
            Some(Cell::Environment { .. })
            | Some(Cell::Iterator { .. })
            | Some(Cell::Proxy { .. })
            | None => Ok(None),
        }
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
