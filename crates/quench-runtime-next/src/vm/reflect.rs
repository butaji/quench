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
                let key = self.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
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
            Native::ReflectPreventExtensions => {
                if self.object_data(target).is_none() {
                    return Err(JsError("Reflect target is not an object".into()));
                }
                self.object_data_mut(target)
                    .expect("object target validated")
                    .set_extensible(false);
                Ok(Value::TRUE)
            }
            Native::ReflectIsExtensible => {
                if self.object_data(target).is_none() {
                    return Err(JsError("Reflect target is not an object".into()));
                }
                Ok(
                    if self.object_data(target).is_some_and(Object::is_extensible) {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    },
                )
            }
            Native::ReflectSet => {
                let key = self.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                let atom = self.intern_atom(&key);
                Ok(
                    if self
                        .set_property(
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
            Native::ReflectOwnKeys => self.object_names(target),
            Native::ReflectGetPrototypeOf => self.object_get_prototype_of(target),
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
                    self.object_set_prototype_of(target, proto)?;
                    Value::TRUE
                })
            }
            Native::ReflectConstruct => {
                let argument_array = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let arguments = if argument_array.is_undefined() {
                    vec![]
                } else {
                    match self.heap.get(argument_array) {
                        Some(Cell::Array { elements, .. }) => elements.as_ref().clone(),
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
