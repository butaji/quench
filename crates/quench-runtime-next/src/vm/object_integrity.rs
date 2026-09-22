use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn object_get_prototype_of(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(value).cloned()
        {
            if handler.is_null() {
                return Err(JsError("cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("getPrototypeOf");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[target])?;
                if !result.is_null() && self.object_data(result).is_none() {
                    return Err(JsError(
                        "proxy getPrototypeOf trap must return an object or null".into(),
                    ));
                }
                if self
                    .object_data(target)
                    .is_some_and(|object| !object.is_extensible() && object.proto != result)
                {
                    return Err(JsError(
                        "proxy getPrototypeOf trap changed a non-extensible target".into(),
                    ));
                }
                return Ok(result);
            }
        }
        let value = self.proxy_target(value);
        self.object_data(value)
            .map(|object| object.proto)
            .ok_or_else(|| JsError("Object.getPrototypeOf target is not an object".into()))
    }

    pub(super) fn object_set_prototype_of(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        proto: Value,
    ) -> Result<Value, JsError> {
        if let Some(Cell::Proxy {
            target: underlying,
            handler,
            ..
        }) = self.heap.get(target).cloned()
        {
            if handler.is_null() {
                return Err(JsError("cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("setPrototypeOf");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[underlying, proto])?;
                if !self.truthy(result) {
                    return Err(JsError("proxy setPrototypeOf trap returned false".into()));
                }
                if self
                    .object_data(underlying)
                    .is_some_and(|object| !object.is_extensible() && object.proto != proto)
                {
                    return Err(JsError(
                        "proxy setPrototypeOf trap changed a non-extensible target".into(),
                    ));
                }
                return Ok(target);
            }
        }
        let target = self.proxy_target(target);
        if !proto.is_null() && self.object_data(proto).is_none() {
            return Err(JsError("Object prototype is not an object".into()));
        }
        let Some(current_proto) = self.object_data(target).map(|object| object.proto) else {
            return Err(JsError(
                "Object.setPrototypeOf target is not an object".into(),
            ));
        };
        if self
            .object_data(target)
            .is_some_and(|object| !object.is_extensible())
            && current_proto != proto
        {
            return Err(JsError(
                "cannot change prototype of non-extensible object".into(),
            ));
        }
        self.object_data_mut(target)
            .expect("object validated")
            .proto = proto;
        Ok(target)
    }

    pub(super) fn integrity_bool(value: bool) -> Value {
        if value { Value::TRUE } else { Value::FALSE }
    }

    pub(super) fn check_property_write(
        &self,
        object: Value,
        atom: Atom,
        exists: bool,
    ) -> Result<(), JsError> {
        if !exists
            && self
                .object_data(object)
                .is_some_and(|object| !object.is_extensible())
        {
            return Err(JsError(
                "cannot add property to non-extensible object".into(),
            ));
        }
        if exists
            && self
                .descriptors
                .get(&(object, atom))
                .is_some_and(|attributes| !attributes.writable)
        {
            return Err(JsError("cannot write non-writable property".into()));
        }
        Ok(())
    }

    pub(super) fn object_prevent_extensions(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(source).cloned()
        {
            if handler.is_null() {
                return Err(JsError("cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("preventExtensions");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[target])?;
                if !self.truthy(result) {
                    return Err(JsError(
                        "proxy preventExtensions trap returned false".into(),
                    ));
                }
                if self.object_data(target).is_some_and(Object::is_extensible) {
                    return Err(JsError(
                        "proxy preventExtensions trap did not make target non-extensible".into(),
                    ));
                }
                return Ok(source);
            }
        }
        let target = self.proxy_target(source);
        if let Some(object) = self.object_data_mut(target) {
            object.set_extensible(false);
        }
        Ok(source)
    }

    pub(super) fn object_is_extensible(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(source).cloned()
        {
            if handler.is_null() {
                return Err(JsError("cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("isExtensible");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[target])?;
                let value = self.truthy(result);
                if self
                    .object_data(target)
                    .is_some_and(|object| object.is_extensible() != value)
                {
                    return Err(JsError(
                        "proxy isExtensible trap disagreed with target".into(),
                    ));
                }
                return Ok(Self::integrity_bool(value));
            }
        }
        let target = self.proxy_target(source);
        let object = self
            .object_data(target)
            .ok_or_else(|| JsError("isExtensible target is not an object".into()))?;
        Ok(Self::integrity_bool(object.is_extensible()))
    }

    pub(super) fn object_set_integrity(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        freeze: bool,
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if matches!(self.heap.get(source), Some(Cell::Proxy { .. })) {
            self.object_prevent_extensions(p, args)?;
            let configurable_atom = self.intern_atom("configurable");
            let writable_atom = self.intern_atom("writable");
            for key in self.object_own_key_values(p, source)? {
                let descriptor = self.object_get_own_property_descriptor(p, &[source, key])?;
                if descriptor.is_undefined() {
                    continue;
                }
                self.set_property(descriptor, configurable_atom, Value::FALSE)?;
                if freeze {
                    self.set_property(descriptor, writable_atom, Value::FALSE)?;
                }
                self.object_define_property(p, &[source, key, descriptor])?;
            }
            if freeze {
                let target = self.proxy_target(source);
                if let Some(object) = self.object_data_mut(target) {
                    object.set_frozen(true);
                }
            }
            return Ok(source);
        }
        let target = self.proxy_target(source);
        if self.object_data(target).is_none() {
            return Ok(target);
        }
        if let Some(object) = self.object_data_mut(target) {
            object.set_extensible(false);
            if freeze {
                object.set_frozen(true);
            }
        }
        let keys = self
            .object_data(target)
            .map(|data| {
                self.ordered_shape(data)
                    .into_iter()
                    .map(|(atom, _)| atom)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for atom in keys {
            let attributes = self
                .descriptors
                .entry((target, atom))
                .or_insert(DEFAULT_PROPERTY_ATTRIBUTES);
            attributes.configurable = false;
            if freeze {
                attributes.writable = false;
            }
        }
        let symbols = self
            .symbol_property_order
            .get(&target)
            .cloned()
            .unwrap_or_default();
        for symbol in symbols {
            let attributes = self
                .symbol_descriptors
                .entry((target, symbol))
                .or_insert(DEFAULT_PROPERTY_ATTRIBUTES);
            attributes.configurable = false;
            if freeze {
                attributes.writable = false;
            }
        }
        Ok(target)
    }

    pub(super) fn object_is_integrity_level(&self, args: &[Value], freeze: bool) -> bool {
        let target = self.proxy_target(args.first().copied().unwrap_or(Value::UNDEFINED));
        let Some(data) = self.object_data(target) else {
            return true;
        };
        if data.is_extensible() || freeze && !data.is_frozen() {
            return false;
        }
        let named_ok = self.ordered_shape(data).into_iter().all(|(atom, _)| {
            let attributes = self
                .descriptors
                .get(&(target, atom))
                .copied()
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            !attributes.configurable && (!freeze || !attributes.writable)
        });
        let symbols_ok = self
            .symbol_property_order
            .get(&target)
            .into_iter()
            .flatten()
            .all(|symbol| {
                let attributes = self
                    .symbol_descriptors
                    .get(&(target, *symbol))
                    .copied()
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                !attributes.configurable && (!freeze || !attributes.writable)
            });
        named_ok && symbols_ok
    }
}
