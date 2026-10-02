use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn call_reflect_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        if native == Native::SuperSet && (target.is_null() || target.is_undefined()) {
            return Err(self.type_error(p, "cannot convert nullish super base to object".into()));
        }
        if matches!(
            native,
            Native::ReflectGet
                | Native::ReflectHas
                | Native::ReflectGetOwnPropertyDescriptor
                | Native::ReflectDefineProperty
                | Native::ReflectDeleteProperty
                | Native::ReflectPreventExtensions
                | Native::ReflectIsExtensible
                | Native::ReflectSet
                | Native::ReflectOwnKeys
                | Native::ReflectGetPrototypeOf
                | Native::ReflectSetPrototypeOf
        ) && !self.is_object_like(target)
        {
            return Err(self.type_error(p, "Reflect target is not an object".into()));
        }
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
                if !self.is_function(target) {
                    return Err(self.type_error(p, "Reflect.apply target is not callable".into()));
                }
                let this = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let list = args.get(2).copied().unwrap_or(Value::UNDEFINED);
                let arguments = self.call_argument_list(p, list, false)?;
                self.call_value(p, target, this, &arguments)
            }
            Native::ReflectGet => {
                let key = self.to_property_key(
                    p,
                    args.get(1).copied().unwrap_or(Value::UNDEFINED),
                )?;
                let receiver = args.get(2).copied().unwrap_or(target);
                if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
                    return self.get_symbol_property_with_receiver(p, target, key, receiver);
                }
                let Some(Cell::String(key)) = self.heap.get(key).cloned() else {
                    unreachable!("ToPropertyKey returns a string or symbol")
                };
                let atom = self.intern_js_atom(&key);
                self.get_property_with_receiver(p, target, atom, receiver)
            }
            Native::ReflectGetOwnPropertyDescriptor => {
                self.object_get_own_property_descriptor(p, args)
            }
            Native::ReflectDefineProperty => self.reflect_define_property(p, args),
            Native::ReflectDeleteProperty => self.object_delete_property(p, args),
            Native::ReflectPreventExtensions => {
                let accepted = self.prevent_extensions(p, target)?;
                Ok(Self::integrity_bool(accepted))
            }
            Native::ReflectIsExtensible => self.object_is_extensible(p, args),
            Native::ReflectSet | Native::SuperSet => {
                let key = self.to_property_key(
                    p,
                    args.get(1).copied().unwrap_or(Value::UNDEFINED),
                )?;
                let receiver = args.get(3).copied().unwrap_or(target);
                let strict_super = native == Native::SuperSet
                    && args.get(4).is_some_and(|flag| self.truthy(*flag));
                if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
                    let succeeded = self.set_symbol_property_with_receiver(
                        p,
                        target,
                        key,
                        args.get(2).copied().unwrap_or(Value::UNDEFINED),
                        receiver,
                    )?;
                    if !succeeded && strict_super {
                        return Err(self.type_error(p, "cannot assign super property".into()));
                    }
                    return Ok(if succeeded { Value::TRUE } else { Value::FALSE });
                }
                let Some(Cell::String(key)) = self.heap.get(key).cloned() else {
                    unreachable!("ToPropertyKey returns a string or symbol")
                };
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
                if proto.is_null() || self.is_object_like(proto) {
                    self.object_set_prototype_of(p, target, proto)?;
                }
                Ok(Value::UNDEFINED)
            }
            Native::ReflectOwnKeys => self.object_own_keys(p, target),
            Native::ReflectGetPrototypeOf => self.object_get_prototype_of(p, target),
            Native::ReflectSetPrototypeOf => {
                let proto = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                self.reflect_set_prototype_of(p, target, proto)
            }
            Native::ReflectConstruct => {
                let new_target = args.get(2).copied().unwrap_or(target);
                if !self.is_constructable(p, target) || !self.is_constructable(p, new_target) {
                    return Err(self.type_error(p, "target is not a constructor".into()));
                }
                let argument_array = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let arguments = self.call_argument_list(p, argument_array, false)?;
                let result =
                    self.construct_value_with_new_target(p, target, new_target, &arguments)?;
                Ok(result)
            }
            _ => Err(JsError("invalid Reflect native".into())),
        }
    }

    pub(super) fn reflect_define_property(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.define_property_from_descriptor(p, args, PropertyDefinitionKind::Reflect)
    }

    fn reflect_set_prototype_of(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        prototype: Value,
    ) -> Result<Value, JsError> {
        if !prototype.is_null() && !self.is_object_like(prototype) {
            return Err(self.type_error(p, "Reflect prototype is not an object".into()));
        }
        let accepted = self.set_prototype_of(p, target, prototype)?;
        Ok(Self::integrity_bool(accepted))
    }
}
