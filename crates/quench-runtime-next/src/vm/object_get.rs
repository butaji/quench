use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn get_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        if object.as_number().is_some() {
            return Ok(if atom == self.primitive_atoms[4] {
                self.native_value(Native::NumberString)
            } else if atom == self.to_fixed_atom {
                self.native_value(Native::NumberFixed)
            } else if atom == self.to_precision_atom {
                self.native_value(Native::NumberPrecision)
            } else {
                Value::UNDEFINED
            });
        }
        let receiver = object;
        let mut object = object;
        loop {
            if let Some(attributes) = self.descriptors.get(&(object, atom)).copied()
                && attributes.accessor
            {
                return match attributes.getter {
                    Some(getter) => self.call_value(p, getter, receiver, &[]),
                    None => Ok(Value::UNDEFINED),
                };
            }
            if let Some(v) = self.own_property(object, atom) {
                return Ok(v);
            }
            if let Some(v) = self.indexed_view_property(object, atom) {
                return Ok(v);
            }
            match self.heap.get(object) {
                Some(Cell::ArrayBuffer { .. })
                    if self.array_buffer_virtual_property(object, atom).is_some() =>
                {
                    return Ok(self.array_buffer_virtual_property(object, atom).unwrap());
                }
                Some(Cell::ArrayBuffer { bytes, shared, .. })
                    if self.lookup_atom("byteLength") == Some(atom) =>
                {
                    let _shared = shared;
                    return Ok(Value::number(if self.array_buffer_detached(object) {
                        0.0
                    } else {
                        bytes.len() as f64
                    }));
                }
                Some(Cell::Array { .. }) if atom == self.length_atom => {
                    let Some(Cell::Array { elements, .. }) = self.heap.get(object) else {
                        unreachable!()
                    };
                    let length = self.heap.sparse_length(object).unwrap_or(elements.len());
                    return Ok(Value::number(length as f64));
                }
                Some(Cell::Map { entries, .. }) if atom == self.size_atom => {
                    return Ok(Value::number(entries.len() as f64));
                }
                Some(Cell::ArrayBuffer { object: x, .. }) => object = x.proto,
                Some(Cell::TypedArray { object: x, .. }) => object = x.proto,
                Some(Cell::DataView { object: x, .. }) => object = x.proto,
                Some(Cell::Set { entries, .. }) if atom == self.size_atom => {
                    return Ok(Value::number(entries.len() as f64));
                }
                Some(Cell::String(v)) => {
                    return Ok(if atom == self.length_atom {
                        Value::number(v.encode_utf16().count() as f64)
                    } else if atom == self.primitive_atoms[0] {
                        self.native_value(Native::StringCharCodeAt)
                    } else if atom == self.primitive_atoms[1] {
                        self.native_value(Native::StringCharAt)
                    } else if atom == self.primitive_atoms[2] {
                        self.native_value(Native::StringSubstring)
                    } else if atom == self.primitive_atoms[3] {
                        self.native_value(Native::StringSubstr)
                    } else if atom == self.primitive_atoms[5] {
                        self.native_value(Native::StringIncludes)
                    } else if atom == self.primitive_atoms[6] {
                        self.native_value(Native::StringStartsWith)
                    } else if atom == self.primitive_atoms[7] {
                        self.native_value(Native::StringEndsWith)
                    } else if let Some(native) = self.string_native_for_atom(atom) {
                        self.native_value(native)
                    } else {
                        Value::UNDEFINED
                    });
                }
                Some(Cell::Date(_)) => return Ok(self.date_property_native(atom)),
                Some(Cell::Object(x)) | Some(Cell::Array { object: x, .. }) => object = x.proto,
                Some(Cell::Map { object: x, .. }) | Some(Cell::Set { object: x, .. }) => {
                    object = x.proto
                }
                Some(Cell::WeakMap { object: x, .. }) | Some(Cell::WeakSet { object: x, .. }) => {
                    object = x.proto
                }
                Some(Cell::WeakRef { object: x, .. }) => object = x.proto,
                Some(Cell::Iterator { object: x, .. }) => object = x.proto,
                Some(Cell::Function { object: x, .. }) => object = x.proto,
                _ => return Ok(Value::UNDEFINED),
            }
            if object.is_null() {
                return Ok(Value::UNDEFINED);
            }
        }
    }
}
