use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    fn proxy_trap(
        &mut self,
        p: &ResidualProgram,
        handler: Value,
        name: &str,
    ) -> Result<Value, JsError> {
        let atom = self.intern_atom(name);
        self.get_property(p, handler, atom)
    }

    pub(super) fn proxy_get(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        if handler.is_null() {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        let trap = self.proxy_trap(p, handler, "get")?;
        if self.is_function(trap) {
            let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
            return self.call_value(p, trap, handler, &[target, key, receiver]);
        }
        self.get_property(p, target, atom)
    }

    pub(super) fn proxy_set(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        atom: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        if handler.is_null() {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        let trap = self.proxy_trap(p, handler, "set")?;

        if let Some(Cell::Proxy {
            handler: current, ..
        }) = self.heap.get(receiver)
            && *current != handler
        {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        if self.is_function(trap) {
            let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
            let result = self.call_value(p, trap, handler, &[target, key, value, receiver])?;
            if !self.truthy(result) {
                return Err(JsError("proxy set trap returned false".into()));
            }
            return Ok(());
        }
        self.set_property_with_program(p, target, atom, value)
    }

    pub(super) fn proxy_get_symbol(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        key: Value,
    ) -> Result<Value, JsError> {
        if handler.is_null() {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        let trap = self.proxy_trap(p, handler, "get")?;
        if self.is_function(trap) {
            return self.call_value(p, trap, handler, &[target, key, receiver]);
        }
        Ok(self
            .symbol_property(target, key)
            .unwrap_or(Value::UNDEFINED))
    }

    pub(super) fn proxy_set_symbol(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        key: Value,
        value: Value,
    ) -> Result<(), JsError> {
        if handler.is_null() {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        let trap = self.proxy_trap(p, handler, "set")?;
        if self.is_function(trap) {
            let result = self.call_value(p, trap, handler, &[target, key, value, receiver])?;
            if !self.truthy(result) {
                return Err(JsError("proxy set trap returned false".into()));
            }
            return Ok(());
        }
        self.set_symbol_property(target, key, value)
    }

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
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = self.heap.get(object).cloned()
            {
                return self.proxy_get(p, target, handler, receiver, atom);
            }
            if let Some(attributes) = self.property_attributes(object, PropertyKey::string(atom))
                && attributes.accessor
            {
                return match attributes.getter {
                    Some(getter) => self.call_value(p, getter, receiver, &[]),
                    None => Ok(Value::UNDEFINED),
                };
            }
            if let Some(index) = super::object_static::array_index(self.atom_name(atom))
                && let Some(Cell::Array { elements, .. }) = self.heap.get(object)
            {
                let value = elements
                    .get(index as usize)
                    .copied()
                    .filter(|value| !value.is_deleted())
                    .or_else(|| self.heap.sparse_get(object, index as usize));
                if let Some(value) = value.filter(|value| !value.is_deleted()) {
                    return Ok(value);
                }
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
                    if let Ok(index) = self.atom_name(atom).parse::<usize>()
                        && let Some(unit) = v.units().get(index).copied()
                    {
                        return Ok(self.heap.alloc(Cell::String(JsString::from_units(&[unit]))));
                    }
                    return Ok(if atom == self.length_atom {
                        Value::number(v.units().len() as f64)
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
                Some(Cell::Symbol(description)) => {
                    let description = description.clone();
                    return Ok(match self.atom_name(atom) {
                        "description" => description
                            .as_ref()
                            .map(|value| self.heap.alloc(Cell::String(value.clone().into())))
                            .unwrap_or(Value::UNDEFINED),
                        "toString" => self.native_value(Native::SymbolToString),
                        "valueOf" => self.native_value(Native::SymbolValueOf),
                        _ => Value::UNDEFINED,
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
                Some(Cell::FinalizationRegistry { object: x, .. }) => object = x.proto,
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
