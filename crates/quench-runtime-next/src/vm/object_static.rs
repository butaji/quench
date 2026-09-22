use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn object_assign(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = args
            .first()
            .copied()
            .filter(|value| self.object_data(*value).is_some())
            .ok_or_else(|| JsError("Object.assign target is not an object".into()))?;
        for source in args.iter().copied().skip(1) {
            let source = self.box_object(source)?;
            let enumerable_atom = self.intern_atom("enumerable");
            for key in self.object_own_key_values(p, source)? {
                let descriptor = self.object_get_own_property_descriptor(p, &[source, key])?;
                if descriptor.is_undefined() {
                    continue;
                }
                let enumerable = self.get_property(p, descriptor, enumerable_atom)?;
                if !self.truthy(enumerable) {
                    continue;
                }
                match self.heap.get(key).cloned() {
                    Some(Cell::Symbol(_)) => {
                        let value = self.get_index(p, source, key)?;
                        self.set_index(p, target, key, value)?;
                    }
                    Some(Cell::String(name)) => {
                        let atom = self.intern_atom(&name);
                        let value = self.get_property(p, source, atom)?;
                        self.set_property_with_program(p, target, atom, value)?;
                    }
                    _ => unreachable!("validated own property key"),
                }
            }
        }
        Ok(target)
    }

    pub(super) fn install_object_extra(
        &mut self,
        program: &ResidualProgram,
        object: Value,
    ) -> Result<(), JsError> {
        self.set_named(
            program,
            self.object_proto,
            "hasOwnProperty",
            self.native_value(Native::ObjectPrototypeHasOwnProperty),
        )?;
        self.set_named(
            program,
            self.object_proto,
            "propertyIsEnumerable",
            self.native_value(Native::ObjectPrototypePropertyIsEnumerable),
        )?;
        self.set_named(
            program,
            self.object_proto,
            "isPrototypeOf",
            self.native_value(Native::ObjectPrototypeIsPrototypeOf),
        )?;
        self.set_named(
            program,
            object,
            "getOwnPropertyNames",
            self.native_value(Native::ObjectGetOwnPropertyNames),
        )?;
        self.set_named(
            program,
            object,
            "getOwnPropertyDescriptor",
            self.native_value(Native::ObjectGetOwnPropertyDescriptor),
        )?;
        self.set_named(
            program,
            object,
            "getOwnPropertySymbols",
            self.native_value(Native::ObjectGetOwnPropertySymbols),
        )?;
        self.set_named(
            program,
            object,
            "getOwnPropertyDescriptors",
            self.native_value(Native::ObjectGetOwnPropertyDescriptors),
        )?;
        self.set_named(
            program,
            object,
            "defineProperty",
            self.native_value(Native::ObjectDefineProperty),
        )?;
        self.set_named(
            program,
            object,
            "values",
            self.native_value(Native::ObjectValues),
        )?;
        self.set_named(
            program,
            object,
            "entries",
            self.native_value(Native::ObjectEntries),
        )?;
        self.set_named(
            program,
            object,
            "fromEntries",
            self.native_value(Native::ObjectFromEntries),
        )?;
        self.set_named(program, object, "is", self.native_value(Native::ObjectIs))
    }

    pub(super) fn call_object_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::ObjectKeys => {
                self.object_keys(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectGetOwnPropertyNames => {
                self.object_names(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectGetOwnPropertySymbols => {
                self.object_symbols(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectGetOwnPropertyDescriptor => {
                self.object_get_own_property_descriptor(p, args)
            }
            Native::ObjectGetOwnPropertyDescriptors => {
                self.object_get_own_property_descriptors(p, args)
            }
            Native::ObjectDefineProperty => self.object_define_property(p, args),
            Native::ObjectValues => {
                self.object_values(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectEntries => {
                self.object_entries(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectFromEntries => {
                let input = args.first().copied().unwrap_or(Value::UNDEFINED);
                let Some(Cell::Array { elements, .. }) = self.heap.get(input).cloned() else {
                    return Err(JsError("Object.fromEntries input is not iterable".into()));
                };
                let object = self.object();
                for entry in elements.iter().copied() {
                    let Some(Cell::Array { elements: pair, .. }) = self.heap.get(entry).cloned()
                    else {
                        return Err(JsError("Object.fromEntries entry is not an array".into()));
                    };
                    let key =
                        self.to_string(p, pair.first().copied().unwrap_or(Value::UNDEFINED))?;
                    let value = pair.get(1).copied().unwrap_or(Value::UNDEFINED);
                    let atom = self.intern_atom(&key);
                    self.set_property(object, atom, value)?;
                }
                Ok(object)
            }
            Native::ObjectIs => {
                let left = args.first().copied().unwrap_or(Value::UNDEFINED);
                let right = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                Ok(if self.same_value(left, right) {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::ObjectCreate => {
                let proto = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !proto.is_null() && self.object_data(proto).is_none() {
                    return Err(JsError("Object prototype is not an object".into()));
                }
                Ok(self.heap.alloc(Cell::Object(Self::empty_object(proto))))
            }
            Native::ObjectAssign => self.object_assign(p, args),
            Native::ObjectGetPrototypeOf => {
                self.object_get_prototype_of(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectSetPrototypeOf => self.object_set_prototype_of(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::ObjectHasOwn => {
                let target = args.first().copied().unwrap_or(Value::UNDEFINED);
                if target.is_null() || target.is_undefined() {
                    return Err(JsError("Object.hasOwn target is nullish".into()));
                }
                let target = self.box_object(target)?;
                let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let descriptor =
                    self.object_get_own_property_descriptor(p, &[target, key_value])?;
                Ok(if !descriptor.is_undefined() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::ObjectPreventExtensions => self.object_prevent_extensions(p, args),
            Native::ObjectIsExtensible => self.object_is_extensible(p, args),
            Native::ObjectSeal => self.object_set_integrity(p, args, false),
            Native::ObjectIsSealed => Ok(Self::integrity_bool(
                self.object_is_integrity_level(args, false),
            )),
            Native::ObjectFreeze => self.object_set_integrity(p, args, true),
            Native::ObjectIsFrozen => Ok(Self::integrity_bool(
                self.object_is_integrity_level(args, true),
            )),
            _ => Err(JsError("invalid object native".into())),
        }
    }

    pub(super) fn same_value(&self, left: Value, right: Value) -> bool {
        if let (Some(a), Some(b)) = (left.as_number(), right.as_number()) {
            return (a.is_nan() && b.is_nan())
                || (a == b && (a != 0.0 || a.is_sign_negative() == b.is_sign_negative()));
        }
        match (self.heap.get(left), self.heap.get(right)) {
            (Some(Cell::String(a)), Some(Cell::String(b)))
            | (Some(Cell::BigInt(a)), Some(Cell::BigInt(b))) => a == b,
            _ => left == right,
        }
    }

    pub(super) fn ordered_shape(&self, data: &Object) -> Vec<(Atom, usize)> {
        let mut entries = self.shapes[data.shape() as usize]
            .iter()
            .copied()
            .enumerate()
            .map(|(slot, atom)| (atom, slot))
            .collect::<Vec<_>>();
        entries.sort_by(|(left, _), (right, _)| {
            match (
                array_index(self.atom_name(*left)),
                array_index(self.atom_name(*right)),
            ) {
                (Some(left), Some(right)) => left.cmp(&right),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
        entries
    }

    pub(super) fn is_enumerable(&self, object: Value, atom: Atom) -> bool {
        let Some(data) = self.object_data(object) else {
            return false;
        };
        let Some(slot) = self.shapes[data.shape() as usize]
            .iter()
            .position(|candidate| *candidate == atom)
        else {
            return false;
        };
        if self.heap.property_get(data, slot).is_none() {
            return false;
        }
        self.descriptors
            .get(&(object, atom))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES)
            .enumerable
    }

    pub(super) fn box_object(&mut self, value: Value) -> Result<Value, JsError> {
        if self.object_data(value).is_some() {
            return Ok(value);
        }
        if value.is_null() || value.is_undefined() {
            return Err(JsError("cannot convert nullish value to object".into()));
        }
        let object = self.object();
        if let Some(Cell::String(text)) = self.heap.get(value).cloned() {
            for (index, unit) in text.encode_utf16().enumerate() {
                let key = self.intern_atom(&index.to_string());
                let character = char::from_u32(u32::from(unit)).unwrap_or('\u{fffd}');
                let value = self.heap.alloc(Cell::String(character.to_string()));
                self.set_property(object, key, value)?;
            }
        }
        Ok(object)
    }

    pub(super) fn object_define_property(
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
            let trap_atom = self.intern_atom("defineProperty");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let key = if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
                    key_value
                } else {
                    let text = self.to_string(p, key_value)?;
                    self.heap.alloc(Cell::String(text))
                };
                let descriptor = args.get(2).copied().unwrap_or(Value::UNDEFINED);
                if self.object_data(descriptor).is_none() {
                    return Err(JsError("property descriptor is not an object".into()));
                }
                let result = self.call_value(p, trap, handler, &[target, key, descriptor])?;
                if !self.truthy(result) {
                    return Err(JsError("proxy defineProperty trap returned false".into()));
                }
                self.validate_proxy_define_property(p, target, key, descriptor)?;
                return Ok(source);
            }
        }
        let target = self.proxy_target(source);
        let target = self
            .object_data(target)
            .map(|_| target)
            .ok_or_else(|| JsError("defineProperty target is not an object".into()))?;
        let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let descriptor = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        if self.object_data(descriptor).is_none() {
            return Err(JsError("property descriptor is not an object".into()));
        }
        if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
            return self.define_symbol_property(target, key_value, descriptor);
        }
        let key = self.to_string(p, key_value)?;
        if let Some(index) = array_index(&key).map(|index| index as usize)
            && matches!(self.heap.get(target), Some(Cell::Array { .. }))
        {
            return self.define_array_property(p, target, index, descriptor);
        }
        let atom = self.intern_atom(&key);
        let existing = self.own_property(target, atom);
        let current = self
            .descriptors
            .get(&(target, atom))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        let is_new = existing.is_none();
        let mut attributes = if is_new {
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            }
        } else {
            current
        };
        for (name, slot) in [
            ("writable", &mut attributes.writable),
            ("enumerable", &mut attributes.enumerable),
            ("configurable", &mut attributes.configurable),
        ] {
            let atom = self.intern_atom(name);
            if let Some(value) = self.own_property(descriptor, atom) {
                *slot = self.truthy(value);
            }
        }
        if !is_new
            && !current.configurable
            && (attributes.configurable != current.configurable
                || attributes.enumerable != current.enumerable
                || attributes.writable && !current.writable)
        {
            return Err(JsError("cannot redefine non-configurable property".into()));
        }
        let value_atom = self.intern_atom("value");
        let descriptor_value = self.own_property(descriptor, value_atom);
        let get_atom = self.intern_atom("get");
        let set_atom = self.intern_atom("set");
        let descriptor_getter = self.own_property(descriptor, get_atom);
        let descriptor_setter = self.own_property(descriptor, set_atom);
        let accessor = descriptor_getter.is_some() || descriptor_setter.is_some();
        if accessor {
            let getter = descriptor_getter
                .map(|value| (!value.is_undefined()).then_some(value))
                .or_else(|| current.accessor.then_some(current.getter))
                .flatten();
            let setter = descriptor_setter
                .map(|value| (!value.is_undefined()).then_some(value))
                .or_else(|| current.accessor.then_some(current.setter))
                .flatten();
            if getter.is_some_and(|value| !self.is_function(value))
                || setter.is_some_and(|value| !self.is_function(value))
            {
                return Err(JsError("property accessor is not callable".into()));
            }
            if !is_new && !current.configurable && !current.accessor {
                return Err(JsError("cannot redefine non-configurable property".into()));
            }
            if is_new {
                self.set_property(target, atom, Value::UNDEFINED)?;
            }
            self.descriptors.insert(
                (target, atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: attributes.enumerable,
                    configurable: attributes.configurable,
                    accessor: true,
                    getter,
                    setter,
                },
            );
            return Ok(target);
        }
        if !is_new
            && !current.configurable
            && !current.writable
            && self
                .own_property(descriptor, value_atom)
                .is_some_and(|next| !self.same_value(existing.unwrap(), next))
        {
            return Err(JsError("cannot write non-writable property".into()));
        }
        let value = descriptor_value.or(existing).unwrap_or(Value::UNDEFINED);
        if is_new || descriptor_value.is_some() && (current.writable || current.configurable) {
            self.set_property(target, atom, value)?;
        }
        self.descriptors.insert((target, atom), attributes);
        Ok(target)
    }
}

pub(super) fn array_index(name: &str) -> Option<u32> {
    if name.is_empty() || name != "0" && name.starts_with('0') {
        return None;
    }
    let index = name.parse::<u32>().ok()?;
    (index.to_string() == name && index < u32::MAX).then_some(index)
}
