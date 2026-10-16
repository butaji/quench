//! Side-effect-free diagnostics for rejected ordinary property assignments.
use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn diagnostic_object_receiver(&mut self, object: Value) -> String {
        let to_string = self.intern_atom("toString");
        let method = self.diagnostic_data_property(object, PropertyKey::string(to_string));
        if method.is_some_and(|method| {
            matches!(
                self.heap.get(method),
                Some(Cell::Function {
                    kind: FunctionKind::Native(Native::ObjectPrototypeToString),
                    ..
                })
            )
        }) {
            return "#<Object>".to_owned();
        }
        let tag = self
            .well_known_symbols
            .get("toStringTag")
            .copied()
            .and_then(|symbol| self.diagnostic_data_property(object, PropertyKey::symbol(symbol)))
            .and_then(|value| match self.heap.get(value) {
                Some(Cell::String(text)) => Some(text.host_string().to_owned()),
                _ => None,
            });
        format!("[object {}]", tag.as_deref().unwrap_or("Object"))
    }
    pub(super) fn nullish_assignment_error(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: &str,
    ) -> JsError {
        let kind = if object.is_null() {
            "null"
        } else {
            "undefined"
        };
        self.type_error(
            p,
            format!("Cannot set properties of {kind} (setting '{key}')"),
        )
    }

    pub(super) fn primitive_assignment_error(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
    ) -> JsError {
        let key = self.atom_name(atom).to_owned();
        let kind = if object.as_bool().is_some() {
            "boolean"
        } else if object.as_number().is_some() {
            "number"
        } else {
            match self.heap.get(object) {
                Some(Cell::String(_)) => "string",
                Some(Cell::Symbol(_)) => "symbol",
                Some(Cell::BigInt(_)) => "bigint",
                _ => "value",
            }
        };
        let value = self.detail_string(object);
        let string_readonly = match self.heap.get(object) {
            Some(Cell::String(text)) => {
                key == "length"
                    || super::object_static::array_index(&key)
                        .is_some_and(|index| (index as usize) < text.len())
            }
            _ => false,
        };
        if string_readonly {
            return self.type_error(
                p,
                format!("Cannot assign to read only property '{key}' of string '{value}'"),
            );
        }
        let mut prototype = self.primitive_prototype(object);
        while let Some(holder) = prototype {
            if self.proxy_parts(holder).is_some() {
                break;
            }
            if let Some(attributes) = self.property_attributes(holder, PropertyKey::string(atom)) {
                if attributes.accessor && attributes.setter.is_none() {
                    let class = match kind {
                        "boolean" => "Boolean",
                        "number" => "Number",
                        "string" => "String",
                        "symbol" => "Symbol",
                        "bigint" => "BigInt",
                        _ => "Object",
                    };
                    return self.type_error(
                        p,
                        format!(
                            "Cannot set property {key} of [object {class}] which has only a getter"
                        ),
                    );
                }
                if !attributes.accessor && !attributes.writable {
                    return self.type_error(
                        p,
                        format!("Cannot assign to read only property '{key}' of {kind} '{value}'"),
                    );
                }
                break;
            }
            prototype = self
                .object_data(holder)
                .map(|data| data.proto())
                .filter(|value| !value.is_null());
        }
        self.type_error(
            p,
            format!("Cannot create property '{key}' on {kind} '{value}'"),
        )
    }

    pub(super) fn rejected_assignment_error(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        define: bool,
    ) -> JsError {
        let key = self.atom_name(atom).to_owned();
        let mut holder = object;
        loop {
            // Diagnostics must not re-enter user accessors or Proxy traps.
            if self.proxy_parts(holder).is_some() {
                break;
            }
            if let Some(attributes) = self.property_attributes(holder, PropertyKey::string(atom)) {
                let receiver = if self.heap.object_record(object).is_some() {
                    self.diagnostic_object_receiver(object)
                } else {
                    self.detail_string(object)
                };
                if attributes.accessor && attributes.setter.is_none() {
                    return self.type_error(
                        p,
                        format!("Cannot set property {key} of {receiver} which has only a getter"),
                    );
                }
                if !attributes.accessor && !attributes.writable {
                    return self.type_error(
                        p,
                        format!(
                            "Cannot assign to read only property '{key}' of object '{receiver}'"
                        ),
                    );
                }
                break;
            }
            let Some(data) = self.object_data(holder) else {
                break;
            };
            holder = data.proto();
            if holder.is_null() {
                break;
            }
        }
        if self
            .object_data(object)
            .is_some_and(|data| !data.is_extensible())
        {
            let verb = if define { "define" } else { "add" };
            return self.type_error(
                p,
                format!("Cannot {verb} property {key}, object is not extensible"),
            );
        }
        self.type_error(p, "cannot assign property".into())
    }
}
