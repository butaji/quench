use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn call_reflect_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        match native {
            Native::ReflectGet => {
                let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
                    return self.get_index(p, target, key_value);
                }
                let key = self.to_string(p, key_value)?;
                let atom = self.intern_atom(&key);
                self.get_property(p, target, atom)
            }
            Native::ReflectGetOwnPropertyDescriptor => {
                self.object_get_own_property_descriptor(p, args)
            }
            Native::ReflectDefineProperty => Ok(if self.object_define_property(p, args).is_ok() {
                Value::TRUE
            } else {
                Value::FALSE
            }),
            Native::ReflectDeleteProperty => self.object_delete_property(p, args),
            Native::ReflectPreventExtensions => {
                if self.object_data(target).is_none() {
                    return Err(JsError("Reflect target is not an object".into()));
                }
                Ok(match self.object_prevent_extensions(p, args) {
                    Ok(_) => Value::TRUE,
                    Err(_) => Value::FALSE,
                })
            }
            Native::ReflectIsExtensible => self.object_is_extensible(p, args),
            Native::ReflectSet => {
                let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
                    return Ok(
                        if self
                            .set_index(
                                p,
                                target,
                                key_value,
                                args.get(2).copied().unwrap_or(Value::UNDEFINED),
                            )
                            .is_ok()
                        {
                            Value::TRUE
                        } else {
                            Value::FALSE
                        },
                    );
                }
                let key = self.to_string(p, key_value)?;
                let atom = self.intern_atom(&key);
                Ok(
                    if self
                        .set_property_with_program(
                            p,
                            target,
                            atom,
                            args.get(2).copied().unwrap_or(Value::UNDEFINED),
                        )
                        .is_ok()
                    {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    },
                )
            }
            Native::ReflectOwnKeys => self.object_own_keys(p, target),
            Native::ReflectGetPrototypeOf => self.object_get_prototype_of(p, target),
            Native::ReflectSetPrototypeOf => {
                let proto = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let Some(object) = self.object_data(target) else {
                    return Err(JsError("Reflect target is not an object".into()));
                };
                if !proto.is_null() && self.object_data(proto).is_none() {
                    return Err(JsError("Reflect prototype is not an object".into()));
                }
                Ok(if !object.is_extensible() && object.proto != proto {
                    Value::FALSE
                } else {
                    match self.object_set_prototype_of(p, target, proto) {
                        Ok(_) => Value::TRUE,
                        Err(_) => Value::FALSE,
                    }
                })
            }
            Native::ReflectConstruct => {
                let argument_array = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let arguments = if argument_array.is_undefined() {
                    vec![]
                } else {
                    match self.heap.get(argument_array) {
                        Some(Cell::Array { elements, .. }) => {
                            super::array::normalized_array_values(elements)
                        }
                        _ => {
                            return Err(JsError(
                                "Reflect.construct arguments must be an array".into(),
                            ));
                        }
                    }
                };
                self.construct_value(p, target, &arguments)
            }
            _ => Err(JsError("invalid Reflect native".into())),
        }
    }
}
