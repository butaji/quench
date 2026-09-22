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

impl<H: Host> Vm<H> {
    pub(super) fn json_parse(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let text = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let parsed: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| JsError(format!("JSON parse: {error}").into()))?;
        self.parse_json_value(&parsed)
    }

    fn parse_json_value(&mut self, value: &serde_json::Value) -> Result<Value, JsError> {
        Ok(match value {
            serde_json::Value::Null => Value::NULL,
            serde_json::Value::Bool(value) => {
                if *value {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            serde_json::Value::Number(value) => Value::number(value.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(value) => self.heap.alloc(Cell::String(value.clone().into())),
            serde_json::Value::Array(values) => {
                let values = values
                    .iter()
                    .map(|value| self.parse_json_value(value))
                    .collect::<Result<Vec<_>, _>>()?;
                self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(values),
                })
            }
            serde_json::Value::Object(values) => {
                let object = self.object();
                for (key, value) in values {
                    let atom = self.intern_atom(key);
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
            | Some(Cell::WeakRef { .. }) => Ok(Some(JsonValue::Object(Vec::new()))),
            Some(Cell::Object(object)) => {
                if ancestors.contains(&value) {
                    return Err(JsError(
                        "JSON.stringify cannot serialize cyclic structures".into(),
                    ));
                }
                ancestors.push(value);
                let shape = object.shape();
                let keys = self.shapes[shape as usize].clone();
                let mut output = Vec::new();
                let result = (|| {
                    for (slot, atom) in keys.into_iter().enumerate() {
                        let Some(value) = self.heap.property_get(&object, slot) else {
                            continue;
                        };
                        if let Some(value) = self.to_json(value, false, ancestors)? {
                            output.push((self.atom_name(atom).into(), value));
                        }
                    }
                    Ok(Some(JsonValue::Object(output)))
                })();
                ancestors.pop();
                result
            }
            Some(Cell::Date(value)) => Ok(self
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
    use super::{JsString, write_json_string};

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
}
