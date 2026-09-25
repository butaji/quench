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
            Native::ReflectHas => {
                let key =
                    self.to_property_key(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                Ok(if self.has_property(p, target, key)? {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::ReflectApply => {
                let this = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let list = args.get(2).copied().unwrap_or(Value::UNDEFINED);
                let arguments = self.call_argument_list(p, list, false)?;
                self.call_value(p, target, this, &arguments)
            }
            Native::ReflectGet => {
                let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let receiver = args.get(2).copied().unwrap_or(target);
                if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
                    return self.get_index(p, target, key_value);
                }
                let key = self.coerce_js_string(p, key_value)?;
                let atom = self.intern_js_atom(&key);
                self.get_property_with_receiver(p, target, atom, receiver)
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
            Native::ReflectSet | Native::SuperSet => {
                if self.object_data(target).is_none() {
                    return Err(self.type_error(p, "Reflect.set target is not an object".into()));
                }
                let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let receiver = args.get(3).copied().unwrap_or(target);
                let strict_super = native == Native::SuperSet
                    && args.get(4).is_some_and(|flag| self.truthy(*flag));
                if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
                    let succeeded = self
                        .set_index(
                            p,
                            target,
                            key_value,
                            args.get(2).copied().unwrap_or(Value::UNDEFINED),
                        )
                        .is_ok();
                    if !succeeded && strict_super {
                        return Err(self.type_error(p, "cannot assign super property".into()));
                    }
                    return Ok(if succeeded { Value::TRUE } else { Value::FALSE });
                }
                let key = self.coerce_js_string(p, key_value)?;
                let atom = self.intern_js_atom(&key);
                let succeeded = self.set_property_with_receiver(
                    p,
                    target,
                    atom,
                    args.get(2).copied().unwrap_or(Value::UNDEFINED),
                    receiver,
                )?;
                if !succeeded && strict_super {
                    return Err(self.type_error(p, "cannot assign super property".into()));
                }
                Ok(if succeeded { Value::TRUE } else { Value::FALSE })
            }
            Native::ObjectLiteralPrototype => {
                let proto = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                if proto.is_null() || self.object_data(proto).is_some() {
                    self.object_set_prototype_of(p, target, proto)?;
                }
                Ok(Value::UNDEFINED)
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
                let new_target = args.get(2).copied().unwrap_or(target);
                if !self.is_constructable(p, target) || !self.is_constructable(p, new_target) {
                    return Err(JsError("target is not a constructor".into()));
                }
                let argument_array = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let arguments = if argument_array.is_undefined() {
                    vec![]
                } else {
                    if !self.is_object_like(argument_array) {
                        return Err(JsError(
                            "Reflect.construct arguments must be an object".into(),
                        ));
                    }
                    self.call_argument_list(p, argument_array, false)?
                };
                let result =
                    self.construct_value_with_new_target(p, target, new_target, &arguments)?;
                Ok(result)
            }
            _ => Err(JsError("invalid Reflect native".into())),
        }
    }
}
