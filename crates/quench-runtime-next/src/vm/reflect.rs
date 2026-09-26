use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn call_reflect_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
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
            Native::ReflectDefineProperty => self.reflect_define_property(p, args),
            Native::ReflectDeleteProperty => self.object_delete_property(p, args),
            Native::ReflectPreventExtensions => self.reflect_prevent_extensions(p, target),
            Native::ReflectIsExtensible => self.object_is_extensible(p, args),
            Native::ReflectSet | Native::SuperSet => {
                let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let receiver = args.get(3).copied().unwrap_or(target);
                let strict_super = native == Native::SuperSet
                    && args.get(4).is_some_and(|flag| self.truthy(*flag));
                if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
                    let succeeded = self.set_symbol_property_with_receiver(
                        p,
                        target,
                        key_value,
                        args.get(2).copied().unwrap_or(Value::UNDEFINED),
                        receiver,
                    )?;
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
                self.reflect_set_prototype_of(p, target, proto)
            }
            Native::ReflectConstruct => {
                let new_target = args.get(2).copied().unwrap_or(target);
                if !self.is_constructable(p, target) || !self.is_constructable(p, new_target) {
                    return Err(self.type_error(p, "target is not a constructor".into()));
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

    fn reflect_define_property(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let key = self.to_property_key(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let descriptor_source = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        let descriptor = self.reflect_to_property_descriptor(p, descriptor_source)?;
        let normalized_args = [source, key, descriptor];
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(source).cloned()
        {
            if handler.is_null() {
                return Err(self.type_error(p, "cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("defineProperty");
            let trap = self.get_property(p, handler, trap_atom)?;
            if trap.is_null() || trap.is_undefined() {
                let forwarded = [target, key, descriptor];
                return self.reflect_define_property(p, &forwarded);
            }
            if !self.is_function(trap) {
                return Err(self.type_error(p, "proxy defineProperty trap is not callable".into()));
            }
            let result = self.call_value(p, trap, handler, &[target, key, descriptor])?;
            if !self.truthy(result) {
                return Ok(Value::FALSE);
            }
            self.validate_proxy_define_property(p, target, key, descriptor)?;
            return Ok(Value::TRUE);
        }
        Ok(
            if self.object_define_property(p, &normalized_args).is_ok() {
                Value::TRUE
            } else {
                Value::FALSE
            },
        )
    }

    fn reflect_to_property_descriptor(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(source) {
            return Err(self.type_error(p, "property descriptor is not an object".into()));
        }
        let descriptor = self.object();
        for name in [
            "enumerable",
            "configurable",
            "value",
            "writable",
            "get",
            "set",
        ] {
            let atom = self.intern_atom(name);
            let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
            if !self.has_property(p, source, key)? {
                continue;
            }
            let value = self.get_property(p, source, atom)?;
            if matches!(name, "get" | "set") && !value.is_undefined() && !self.is_function(value) {
                return Err(self.type_error(
                    p,
                    format!("property descriptor {name} field is not callable"),
                ));
            }
            self.set_property(descriptor, atom, value)?;
        }
        let data = ["value", "writable"].iter().any(|name| {
            let atom = self.intern_atom(name);
            self.own_property(descriptor, atom).is_some()
        });
        let accessor = ["get", "set"].iter().any(|name| {
            let atom = self.intern_atom(name);
            self.own_property(descriptor, atom).is_some()
        });
        if data && accessor {
            return Err(self.type_error(
                p,
                "property descriptor mixes data and accessor fields".into(),
            ));
        }
        Ok(descriptor)
    }

    fn reflect_set_prototype_of(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        prototype: Value,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(target) {
            return Err(self.type_error(p, "Reflect target is not an object".into()));
        }
        if !prototype.is_null() && !self.is_object_like(prototype) {
            return Err(self.type_error(p, "Reflect prototype is not an object".into()));
        }
        if let Some(Cell::Proxy {
            target: underlying,
            handler,
            ..
        }) = self.heap.get(target).cloned()
        {
            if handler.is_null() {
                return Err(self.type_error(p, "cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("setPrototypeOf");
            let trap = self.get_property(p, handler, trap_atom)?;
            if trap.is_null() || trap.is_undefined() {
                return self.reflect_set_prototype_of(p, underlying, prototype);
            }
            if !self.is_function(trap) {
                return Err(self.type_error(p, "proxy setPrototypeOf trap is not callable".into()));
            }
            let result = self.call_value(p, trap, handler, &[underlying, prototype])?;
            if !self.truthy(result) {
                return Ok(Value::FALSE);
            }
            let extensible = self.object_is_extensible(p, &[underlying])?;
            if !self.truthy(extensible) {
                let target_prototype = self.object_get_prototype_of(p, underlying)?;
                if !self.same_value(target_prototype, prototype) {
                    return Err(self.type_error(
                        p,
                        "proxy setPrototypeOf trap changed a non-extensible target".into(),
                    ));
                }
            }
            return Ok(Value::TRUE);
        }
        let current = self.object_get_prototype_of(p, target)?;
        if self.same_value(current, prototype) {
            return Ok(Value::TRUE);
        }
        let extensible = self.object_is_extensible(p, &[target])?;
        if !self.truthy(extensible) {
            return Ok(Value::FALSE);
        }
        let mut cursor = prototype;
        while !cursor.is_null() {
            if self.same_value(cursor, target) {
                return Ok(Value::FALSE);
            }
            cursor = self.object_get_prototype_of(p, cursor)?;
        }
        self.object_set_prototype_of(p, target, prototype)?;
        Ok(Value::TRUE)
    }

    fn reflect_prevent_extensions(
        &mut self,
        p: &ResidualProgram,
        target: Value,
    ) -> Result<Value, JsError> {
        let Some(Cell::Proxy {
            target: underlying,
            handler,
            ..
        }) = self.heap.get(target).cloned()
        else {
            self.object_prevent_extensions(p, &[target])?;
            return Ok(Value::TRUE);
        };
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        let trap_atom = self.intern_atom("preventExtensions");
        let trap = self.get_property(p, handler, trap_atom)?;
        if trap.is_null() || trap.is_undefined() {
            return self.reflect_prevent_extensions(p, underlying);
        }
        if !self.is_function(trap) {
            return Err(self.type_error(p, "proxy preventExtensions trap is not callable".into()));
        }
        let result = self.call_value(p, trap, handler, &[underlying])?;
        if !self.truthy(result) {
            return Ok(Value::FALSE);
        }
        let extensible = self.object_is_extensible(p, &[underlying])?;
        if self.truthy(extensible) {
            return Err(self.type_error(
                p,
                "proxy preventExtensions trap did not make target non-extensible".into(),
            ));
        }
        Ok(Value::TRUE)
    }
}
