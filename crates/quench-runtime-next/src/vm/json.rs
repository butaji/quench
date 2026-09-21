use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn json_parse(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let text = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let parsed: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| JsError(format!("JSON parse: {error}").into()))?;
        self.from_json(&parsed)
    }

    fn from_json(&mut self, value: &serde_json::Value) -> Result<Value, JsError> {
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
            serde_json::Value::String(value) => self.heap.alloc(Cell::String(value.clone())),
            serde_json::Value::Array(values) => {
                let values = values
                    .iter()
                    .map(|value| self.from_json(value))
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
                    let value = self.from_json(value)?;
                    self.set_property(object, atom, value)?;
                }
                object
            }
        })
    }

    pub(super) fn json_stringify(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let Some(value) = self.to_json(p, value, false)? else {
            return Ok(Value::UNDEFINED);
        };
        let text = serde_json::to_string(&value)
            .map_err(|error| JsError(format!("JSON stringify: {error}").into()))?;
        Ok(self.heap.alloc(Cell::String(text)))
    }

    fn to_json(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        array_element: bool,
    ) -> Result<Option<serde_json::Value>, JsError> {
        if value.is_undefined() {
            return Ok(array_element.then_some(serde_json::Value::Null));
        }
        if value.is_null() {
            return Ok(Some(serde_json::Value::Null));
        }
        if let Some(value) = value.as_bool() {
            return Ok(Some(serde_json::Value::Bool(value)));
        }
        if let Some(value) = value.as_number() {
            let number = if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value <= i64::MAX as f64
            {
                serde_json::Number::from(value as i64)
            } else {
                serde_json::Number::from_f64(value).unwrap_or_else(|| serde_json::Number::from(0))
            };
            return Ok(Some(serde_json::Value::Number(number)));
        }
        match self.heap.get(value).cloned() {
            Some(Cell::String(value)) => Ok(Some(serde_json::Value::String(value))),
            Some(Cell::BigInt(_)) | Some(Cell::Symbol(_)) => {
                Err(JsError("JSON cannot stringify this value".into()))
            }
            Some(Cell::Function { .. }) => Ok(None),
            Some(Cell::Array { elements, .. }) => {
                let mut output = Vec::with_capacity(elements.len());
                for value in elements.iter().copied() {
                    output.push(
                        self.to_json(p, value, true)?
                            .unwrap_or(serde_json::Value::Null),
                    );
                }
                Ok(Some(serde_json::Value::Array(output)))
            }
            Some(Cell::Map { .. })
            | Some(Cell::Set { .. })
            | Some(Cell::WeakMap { .. })
            | Some(Cell::WeakSet { .. })
            | Some(Cell::WeakRef { .. }) => {
                Ok(Some(serde_json::Value::Object(serde_json::Map::new())))
            }
            Some(Cell::Object(object)) => {
                let shape = object.shape();
                let keys = self.shapes[shape as usize].clone();
                let mut output = serde_json::Map::new();
                for (slot, atom) in keys.into_iter().enumerate() {
                    let Some(value) = self.heap.property_get(&object, slot) else {
                        continue;
                    };
                    if let Some(value) = self.to_json(p, value, false)? {
                        output.insert(self.atom_name(atom).into(), value);
                    }
                }
                Ok(Some(serde_json::Value::Object(output)))
            }
            Some(Cell::Date(value)) => Ok(serde_json::Number::from_f64(value)
                .map(serde_json::Value::Number)
                .or_else(|| Some(serde_json::Value::Null))),
            Some(Cell::Error(value)) => Ok(Some(serde_json::Value::String(value))),
            Some(Cell::Environment { .. }) | Some(Cell::Iterator { .. }) | None => Ok(None),
        }
    }
}
