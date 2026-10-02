use super::object_descriptors::PropertyDescriptorRecord;
use super::property_key::PropertyKey;
use super::*;
impl<H: Host> Vm<H> {
    pub(super) fn delete_name(
        &mut self,
        p: &ResidualProgram,
        atom: Atom,
    ) -> Result<Value, JsError> {
        if self.atom_name(atom).starts_with('\0') {
            return Ok(Value::TRUE);
        }
        let frame = self.frames.len().saturating_sub(1);
        let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
        let with_base = self
            .frames
            .get(frame)
            .map_or(self.with_stack.len(), |frame| frame.with_base)
            .min(self.with_stack.len());
        let with_objects = self.with_stack[with_base..].to_vec();
        for object in with_objects.into_iter().rev() {
            if self.with_binding(p, object, key, atom)? {
                return self.object_delete_property(p, &[object, key]);
            }
        }
        if let Some(deleted) = self.delete_environment_binding(p, frame, atom) {
            return Ok(if deleted { Value::TRUE } else { Value::FALSE });
        }
        if self.realm.global_lexical_bindings.contains_key(&atom) {
            return Ok(Value::FALSE);
        }
        self.object_delete_property(p, &[self.realm.globals, key])
    }

    pub(super) fn object_delete_property(
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
            let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
            let key = self.to_property_key(p, key_value)?;
            let trap_atom = self.intern_atom("deleteProperty");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[target, key])?;
                if !self.truthy(result) {
                    return Ok(Value::FALSE);
                }
                let descriptor = self.object_get_own_property_descriptor(p, &[target, key])?;
                if !descriptor.is_undefined() && !self.descriptor_flag(descriptor, "configurable") {
                    return Err(self.type_error(
                        p,
                        "proxy deleteProperty trap cannot delete a non-configurable property"
                            .into(),
                    ));
                }
                let extensible = if descriptor.is_undefined() {
                    Value::TRUE
                } else {
                    self.object_is_extensible(p, &[target])?
                };
                if !descriptor.is_undefined() && !self.truthy(extensible) {
                    return Err(self.type_error(
                        p,
                        "proxy deleteProperty trap cannot hide a property of a non-extensible target"
                            .into(),
                    ));
                }
                return Ok(Value::TRUE);
            } else if !trap.is_null() && !trap.is_undefined() {
                return Err(self.type_error(p, "proxy deleteProperty trap is not callable".into()));
            } else {
                let mut forwarded = args.to_vec();
                if let Some(receiver) = forwarded.first_mut() {
                    *receiver = target;
                }
                return self.object_delete_property(p, &forwarded);
            }
        }
        let target = self.proxy_target(source);
        if self.object_data(target).is_none() {
            return Err(JsError("delete target is not an object".into()));
        }
        let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let key = self.to_property_key(p, key_value)?;
        let property_key = if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            PropertyKey::symbol(key)
        } else {
            let Some(Cell::String(key)) = self.heap.get(key).cloned() else {
                unreachable!("ToPropertyKey returns a string or symbol")
            };
            let atom = self.intern_js_atom(&key);
            self.evaluate_deferred_namespace_for_key(p, target, Some(PropertyKey::string(atom)))?;
            if key.host_string() == "length"
                && matches!(self.heap.get(target), Some(Cell::Array { .. }))
                && !self
                    .object_data(target)
                    .is_some_and(Object::is_arguments_object)
            {
                return Ok(Value::FALSE);
            }
            if matches!(self.heap.get(target), Some(Cell::TypedArray { .. })) {
                match Self::typed_array_index_key(key.host_string()) {
                    super::object_descriptors::TypedArrayIndexKey::Index(index) => {
                        return Ok(if self
                            .typed_array_length(target)
                            .is_some_and(|length| index < length)
                        {
                            Value::FALSE
                        } else {
                            Value::TRUE
                        });
                    }
                    super::object_descriptors::TypedArrayIndexKey::Invalid => {
                        return Ok(Value::TRUE);
                    }
                    super::object_descriptors::TypedArrayIndexKey::NotCanonical => {}
                }
            }
            if let Some(index) =
                super::object_static::array_index(key.host_string()).map(|index| index as usize)
                && matches!(self.heap.get(target), Some(Cell::Array { .. }))
            {
                return Ok(self.delete_array_index(target, index));
            }
            PropertyKey::string(atom)
        };
        let Some((_, slot)) =
            self.object_property_slot(target, property_key)
                .filter(|(_, slot)| {
                    self.object_data(target)
                        .and_then(|object| self.heap.property_get(object, *slot))
                        .is_some()
                })
        else {
            return Ok(Value::TRUE);
        };
        if !self
            .property_attributes(target, property_key)
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES)
            .configurable
        {
            return Ok(Value::FALSE);
        }
        self.heap.property_set(target, slot, Value::DELETED);
        self.delete_shape_property(target, property_key);
        Ok(Value::TRUE)
    }

    pub(super) fn delete_reference_property(
        &mut self,
        p: &ResidualProgram,
        base: Value,
        key: Value,
    ) -> Result<Value, JsError> {
        if base.is_null() || base.is_undefined() {
            return Err(self.type_error(p, "Cannot convert undefined or null to object".into()));
        }
        let target = self.box_object(base)?;
        self.object_delete_property(p, &[target, key])
    }

    pub(super) fn object_get_prototype_of(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(value).cloned()
        {
            if handler.is_null() {
                return Err(self.type_error(p, "cannot access a revoked proxy".into()));
            }
            let proxy = self.heap.root(value);
            let target = self.heap.root(target);
            let handler = self.heap.root(handler);
            let mut result_root = None;
            let outcome = (|| {
                let atom = self.intern_atom("getPrototypeOf");
                let trap = self.get_property(p, self.heap.root_value(handler).unwrap(), atom)?;
                if trap.is_null() || trap.is_undefined() {
                    return self.object_get_prototype_of(p, self.heap.root_value(target).unwrap());
                }
                if !self.is_function(trap) {
                    return Err(self.type_error(p, "proxy getPrototypeOf trap is not callable".into()));
                }
                let result = self.call_value(
                    p,
                    trap,
                    self.heap.root_value(handler).unwrap(),
                    &[self.heap.root_value(target).unwrap()],
                )?;
                if !result.is_null() && !self.is_object_like(result) {
                    return Err(self.type_error(
                        p,
                        "proxy getPrototypeOf trap must return an object or null".into(),
                    ));
                }
                let result = self.heap.root(result);
                result_root = Some(result);
                let extensible =
                    self.object_is_extensible(p, &[self.heap.root_value(target).unwrap()])?;
                if !self.truthy(extensible) {
                    let target_prototype =
                        self.object_get_prototype_of(p, self.heap.root_value(target).unwrap())?;
                    if !self.same_value(target_prototype, self.heap.root_value(result).unwrap()) {
                        return Err(self.type_error(
                            p,
                            "proxy getPrototypeOf trap changed a non-extensible target".into(),
                        ));
                    }
                }
                Ok(self.heap.root_value(result).unwrap())
            })();
            for root in [Some(proxy), Some(target), Some(handler), result_root]
                .into_iter()
                .flatten()
            {
                self.heap.release_root(root);
            }
            return outcome;
        }
        let value = self.box_object_or_type_error(p, value)?;
        Ok(self
            .object_data(value)
            .map(|object| object.proto)
            .unwrap_or(Value::NULL))
    }

    pub(super) fn object_set_prototype_of(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        proto: Value,
    ) -> Result<Value, JsError> {
        self.require_object_coercible(p, target)?;
        if !proto.is_null() && !self.is_object_like(proto) {
            return Err(self.type_error(p, "Object prototype is not an object".into()));
        }
        if !self.is_object_like(target) {
            return Ok(target);
        }
        let target = self.heap.root(target);
        let outcome = (|| {
            if !self.set_prototype_of(p, self.heap.root_value(target).unwrap(), proto)? {
                return Err(self.type_error(p, "cannot set object prototype".into()));
            }
            Ok(self.heap.root_value(target).unwrap())
        })();
        self.heap.release_root(target);
        outcome
    }

    pub(super) fn set_prototype_of(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        proto: Value,
    ) -> Result<bool, JsError> {
        let _stack = self.enter_stack()?;
        if let Some(Cell::Proxy {
            target: underlying,
            handler,
            ..
        }) = self.heap.get(target).cloned()
        {
            if handler.is_null() {
                return Err(self.type_error(p, "cannot access a revoked proxy".into()));
            }
            let proxy = self.heap.root(target);
            let underlying = self.heap.root(underlying);
            let handler = self.heap.root(handler);
            let proto = self.heap.root(proto);
            let outcome = (|| {
                let atom = self.intern_atom("setPrototypeOf");
                let trap = self.get_property(p, self.heap.root_value(handler).unwrap(), atom)?;
                if trap.is_null() || trap.is_undefined() {
                    return self.set_prototype_of(
                        p,
                        self.heap.root_value(underlying).unwrap(),
                        self.heap.root_value(proto).unwrap(),
                    );
                }
                if !self.is_function(trap) {
                    return Err(self.type_error(p, "proxy setPrototypeOf trap is not callable".into()));
                }
                let result = self.call_value(
                    p,
                    trap,
                    self.heap.root_value(handler).unwrap(),
                    &[
                        self.heap.root_value(underlying).unwrap(),
                        self.heap.root_value(proto).unwrap(),
                    ],
                )?;
                if !self.truthy(result) {
                    return Ok(false);
                }
                let extensible =
                    self.object_is_extensible(p, &[self.heap.root_value(underlying).unwrap()])?;
                if !self.truthy(extensible) {
                    let current =
                        self.object_get_prototype_of(p, self.heap.root_value(underlying).unwrap())?;
                    if !self.same_value(current, self.heap.root_value(proto).unwrap()) {
                        return Err(self.type_error(
                            p,
                            "proxy setPrototypeOf trap changed a non-extensible target".into(),
                        ));
                    }
                }
                Ok(true)
            })();
            for root in [proxy, underlying, handler, proto] {
                self.heap.release_root(root);
            }
            return outcome;
        }
        let data = self
            .object_data(target)
            .expect("SetPrototypeOf target is an object");
        if self.same_value(data.proto, proto) {
            return Ok(true);
        }
        if target == self.object_proto || !data.is_extensible() {
            return Ok(false);
        }
        let mut cursor = proto;
        while !cursor.is_null() {
            if self.same_value(cursor, target) {
                return Ok(false);
            }
            if matches!(self.heap.get(cursor), Some(Cell::Proxy { .. })) {
                break;
            }
            cursor = self
                .object_data(cursor)
                .map(|object| object.proto)
                .unwrap_or(Value::NULL);
        }
        self.object_data_mut(target)
            .expect("object validated")
            .proto = proto;
        self.mark_object_dictionary(target, DictionaryTrigger::PrototypeUse);
        self.invalidate_field_caches();
        self.invalidate_method_caches();
        Ok(true)
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
        self.check_property_key_write(object, PropertyKey::string(atom), exists)
    }

    pub(super) fn check_property_key_write(
        &self,
        object: Value,
        key: PropertyKey,
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
                .property_attributes(object, key)
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
        if !self.is_object_like(source) {
            return Ok(source);
        }
        let source = self.heap.root(source);
        let outcome = (|| {
            if !self.prevent_extensions(p, self.heap.root_value(source).unwrap())? {
                return Err(self.type_error(p, "cannot prevent object extensions".into()));
            }
            Ok(self.heap.root_value(source).unwrap())
        })();
        self.heap.release_root(source);
        outcome
    }

    pub(super) fn prevent_extensions(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<bool, JsError> {
        let _stack = self.enter_stack()?;
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(source).cloned()
        {
            if handler.is_null() {
                return Err(self.type_error(p, "cannot access a revoked proxy".into()));
            }
            let proxy = self.heap.root(source);
            let target = self.heap.root(target);
            let handler = self.heap.root(handler);
            let outcome = (|| {
                let atom = self.intern_atom("preventExtensions");
                let trap = self.get_property(p, self.heap.root_value(handler).unwrap(), atom)?;
                if trap.is_null() || trap.is_undefined() {
                    return self.prevent_extensions(p, self.heap.root_value(target).unwrap());
                }
                if !self.is_function(trap) {
                    return Err(
                        self.type_error(p, "proxy preventExtensions trap is not callable".into())
                    );
                }
                let result = self.call_value(
                    p,
                    trap,
                    self.heap.root_value(handler).unwrap(),
                    &[self.heap.root_value(target).unwrap()],
                )?;
                if !self.truthy(result) {
                    return Ok(false);
                }
                let extensible =
                    self.object_is_extensible(p, &[self.heap.root_value(target).unwrap()])?;
                if self.truthy(extensible) {
                    return Err(self.type_error(
                        p,
                        "proxy preventExtensions trap did not make target non-extensible".into(),
                    ));
                }
                Ok(true)
            })();
            for root in [proxy, target, handler] {
                self.heap.release_root(root);
            }
            return outcome;
        }
        if self.typed_array_length_is_variable(source) {
            return Ok(false);
        }
        self.object_data_mut(source)
            .expect("PreventExtensions target is an object")
            .set_extensible(false);
        Ok(true)
    }

    pub(super) fn object_is_extensible(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(source).cloned()
        {
            if handler.is_null() {
                return Err(self.type_error(p, "cannot access a revoked proxy".into()));
            }
            let proxy = self.heap.root(source);
            let target = self.heap.root(target);
            let handler = self.heap.root(handler);
            let outcome = (|| {
                let atom = self.intern_atom("isExtensible");
                let trap = self.get_property(p, self.heap.root_value(handler).unwrap(), atom)?;
                if trap.is_null() || trap.is_undefined() {
                    return self.object_is_extensible(p, &[self.heap.root_value(target).unwrap()]);
                }
                if !self.is_function(trap) {
                    return Err(self.type_error(p, "proxy isExtensible trap is not callable".into()));
                }
                let result = self.call_value(
                    p,
                    trap,
                    self.heap.root_value(handler).unwrap(),
                    &[self.heap.root_value(target).unwrap()],
                )?;
                let result = self.truthy(result);
                let target_result =
                    self.object_is_extensible(p, &[self.heap.root_value(target).unwrap()])?;
                if self.truthy(target_result) != result {
                    return Err(
                        self.type_error(p, "proxy isExtensible trap disagreed with target".into())
                    );
                }
                Ok(Self::integrity_bool(result))
            })();
            for root in [proxy, target, handler] {
                self.heap.release_root(root);
            }
            return outcome;
        }
        Ok(Self::integrity_bool(
            self.object_data(source).is_some_and(Object::is_extensible),
        ))
    }

    fn set_integrity_level(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        freeze: bool,
    ) -> Result<Value, JsError> {
        let object = self.heap.root(object);
        let mut keys = Vec::new();
        let outcome = (|| {
            let target = self.heap.root_value(object).unwrap();
            self.object_prevent_extensions(p, &[target])?;
            let target = self.heap.root_value(object).unwrap();
            keys = self
                .object_own_key_values(p, target)?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            for &key in &keys {
                let is_data = if freeze {
                    let target = self.heap.root_value(object).unwrap();
                    let property = self.heap.root_value(key).unwrap();
                    let current = self.object_get_own_property_descriptor(p, &[target, property])?;
                    if current.is_undefined() {
                        continue;
                    }
                    self.own_descriptor_record(current).has_data_fields()
                } else {
                    false
                };
                let record = PropertyDescriptorRecord {
                    value: None,
                    writable: is_data.then_some(false),
                    enumerable: None,
                    configurable: Some(false),
                    getter: None,
                    setter: None,
                };
                let target = self.heap.root_value(object).unwrap();
                let property = self.heap.root_value(key).unwrap();
                self.define_property_or_throw(p, target, property, record)?;
            }
            Ok(self.heap.root_value(object).unwrap())
        })();
        for root in keys {
            self.heap.release_root(root);
        }
        self.heap.release_root(object);
        outcome
    }

    fn test_integrity_level(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        freeze: bool,
    ) -> Result<Value, JsError> {
        let object = self.heap.root(object);
        let mut keys = Vec::new();
        let outcome = (|| {
            let target = self.heap.root_value(object).unwrap();
            let extensible = self.object_is_extensible(p, &[target])?;
            if self.truthy(extensible) {
                return Ok(Value::FALSE);
            }
            let target = self.heap.root_value(object).unwrap();
            keys = self
                .object_own_key_values(p, target)?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            for &key in &keys {
                let target = self.heap.root_value(object).unwrap();
                let property = self.heap.root_value(key).unwrap();
                let current = self.object_get_own_property_descriptor(p, &[target, property])?;
                if current.is_undefined() {
                    continue;
                }
                let record = self.own_descriptor_record(current);
                if record.configurable == Some(true)
                    || (freeze && record.has_data_fields() && record.writable == Some(true))
                {
                    return Ok(Value::FALSE);
                }
            }
            Ok(Value::TRUE)
        })();
        for root in keys {
            self.heap.release_root(root);
        }
        self.heap.release_root(object);
        outcome
    }

    pub(super) fn object_set_integrity(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        freeze: bool,
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if freeze && let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(source) {
            let resizable = matches!(
                self.heap.get(*buffer),
                Some(Cell::ArrayBuffer {
                    resizable: true,
                    ..
                })
            );
            if resizable
                || self
                    .typed_array_length(source)
                    .is_some_and(|length| length > 0)
            {
                return Err(self.type_error(
                    p,
                    "cannot freeze a typed array with indexed elements".into(),
                ));
            }
        }
        if matches!(self.heap.get(source), Some(Cell::Proxy { .. }))
            || self.object_data(source).is_some_and(Object::is_module_namespace)
        {
            return self.set_integrity_level(p, source, freeze);
        }
        let target = source;
        if !freeze && matches!(self.heap.get(target), Some(Cell::TypedArray { .. })) {
            self.object_prevent_extensions(p, &[target])?;
            if self.typed_array_length(target).is_some_and(|length| length > 0) {
                return Err(self.type_error(
                    p,
                    "cannot seal a typed array with indexed elements".into(),
                ));
            }
            return Ok(target);
        }
        if self.object_data(target).is_none() {
            return Ok(target);
        }
        if let Some(object) = self.object_data_mut(target) {
            object.set_extensible(false);
            if freeze {
                object.set_frozen(true);
            }
        }
        let mut keys = self
            .object_data(target)
            .map(|data| {
                self.ordered_shape(data)
                    .into_iter()
                    .filter(|(_, slot)| self.heap.property_get(data, *slot).is_some())
                    .map(|(atom, _)| atom)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        keys.extend(self.array_integrity_atoms(target));
        for atom in keys {
            let mut attributes = self
                .property_attributes(target, PropertyKey::string(atom))
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            attributes.configurable = false;
            if freeze {
                attributes.writable = false;
            }
            self.set_property_attributes(target, PropertyKey::string(atom), attributes);
        }
        let symbols = self
            .object_data(target)
            .map(|data| self.shape_keys(data.shape()))
            .unwrap_or_default()
            .into_iter()
            .filter(|key| matches!(key, PropertyKey::Symbol(_)))
            .collect::<Vec<_>>();
        for symbol in symbols {
            let mut attributes = self
                .property_attributes(target, symbol)
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            attributes.configurable = false;
            if freeze {
                attributes.writable = false;
            }
            self.set_property_attributes(target, symbol, attributes);
        }
        Ok(target)
    }

    pub(super) fn object_is_integrity_level(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        freeze: bool,
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if matches!(self.heap.get(source), Some(Cell::Proxy { .. }))
            || self.object_data(source).is_some_and(Object::is_module_namespace)
        {
            return self.test_integrity_level(p, source, freeze);
        }
        let target = source;
        let Some(data) = self.object_data(target) else {
            return Ok(Value::TRUE);
        };
        if data.is_extensible() {
            return Ok(Value::FALSE);
        }
        let named_ok = self
            .ordered_shape(data)
            .into_iter()
            .filter(|(_, slot)| self.heap.property_get(data, *slot).is_some())
            .all(|(atom, _)| {
                let attributes = self
                    .property_attributes(target, PropertyKey::string(atom))
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                !attributes.configurable && (!freeze || !attributes.writable)
            });
        let arrays_ok = self.array_is_integrity_level(target, freeze);
        let shape_keys = self.shape_keys(data.shape());
        let symbols_ok = shape_keys
            .iter()
            .filter(|key| matches!(key, PropertyKey::Symbol(_)))
            .all(|symbol| {
                let attributes = self
                    .property_attributes(target, *symbol)
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                !attributes.configurable && (!freeze || !attributes.writable)
            });
        Ok(Self::integrity_bool(named_ok && arrays_ok && symbols_ok))
    }

    pub(super) fn validate_proxy_define_property_record(
        &mut self,
        p: &ResidualProgram,
        target: RootId,
        key: RootId,
        descriptor: &super::property_definition::RootedPropertyDescriptor,
    ) -> Result<(), JsError> {
        let object = self.heap.root_value(target).unwrap();
        let property = self.heap.root_value(key).unwrap();
        let current = self.object_get_own_property_descriptor(p, &[object, property])?;
        let current = if current.is_undefined() {
            None
        } else {
            let record = self.own_descriptor_record(current);
            Some(super::property_definition::RootedPropertyDescriptor::new(
                &mut self.heap,
                record,
            ))
        };
        let outcome = (|| {
            let object = self.heap.root_value(target).unwrap();
            let extensible = self.object_is_extensible(p, &[object])?;
            let record = descriptor.resolve(&self.heap);
            let current = current.as_ref().map(|r| r.resolve(&self.heap));
            if !record.compatible_with(current, self.truthy(extensible), |a, b| {
                self.same_value(a, b)
            }) {
                return Err(self.type_error(
                    p,
                    "proxy defineProperty trap returned an incompatible descriptor".into(),
                ));
            }
            if record.configurable == Some(false)
                && current.is_none_or(|r| r.configurable == Some(true))
            {
                return Err(self.type_error(
                    p,
                    "proxy defineProperty trap introduced a non-configurable property".into(),
                ));
            }
            if current.is_some_and(|r| {
                r.has_data_fields() && r.configurable == Some(false) && r.writable == Some(true)
            }) && record.writable == Some(false)
            {
                return Err(self.type_error(
                    p,
                    "proxy defineProperty trap changed a non-writable target property".into(),
                ));
            }
            Ok(())
        })();
        if let Some(current) = current {
            current.release(&mut self.heap);
        }
        outcome
    }

}
