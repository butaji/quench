use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn object_get_prototype_of(&self, value: Value) -> Result<Value, JsError> {
        self.object_data(value)
            .map(|object| object.proto)
            .ok_or_else(|| JsError("Object.getPrototypeOf target is not an object".into()))
    }

    pub(super) fn object_set_prototype_of(
        &mut self,
        target: Value,
        proto: Value,
    ) -> Result<Value, JsError> {
        if !proto.is_null() && self.object_data(proto).is_none() {
            return Err(JsError("Object prototype is not an object".into()));
        }
        let Some(object) = self.object_data_mut(target) else {
            return Err(JsError(
                "Object.setPrototypeOf target is not an object".into(),
            ));
        };
        object.proto = proto;
        Ok(target)
    }

    pub(super) fn object_assign(&mut self, args: &[Value]) -> Result<Value, JsError> {
        let target = args
            .first()
            .copied()
            .filter(|value| self.object_data(*value).is_some())
            .ok_or_else(|| JsError("Object.assign target is not an object".into()))?;
        for source in args.iter().copied().skip(1) {
            let source = self.box_object(source)?;
            let data = self.object_data(source).expect("boxed source is object");
            let values = self
                .ordered_shape(data)
                .into_iter()
                .filter_map(|(atom, slot)| {
                    if !self.is_enumerable(source, atom) {
                        return None;
                    }
                    self.heap
                        .property_get(data, slot)
                        .map(|value| (atom, value))
                })
                .collect::<Vec<_>>();
            for (atom, value) in values {
                self.set_property(target, atom, value)?;
            }
        }
        Ok(target)
    }

    pub(super) fn object_keys(&mut self, object: Value) -> Result<Value, JsError> {
        let object = self.box_object(object)?;
        let data = self.object_data(object).expect("boxed target is object");
        let atoms = self
            .ordered_shape(data)
            .into_iter()
            .filter(|(atom, _)| self.is_enumerable(object, *atom))
            .map(|(atom, _)| atom)
            .collect::<Vec<_>>();
        let values = atoms
            .into_iter()
            .map(|atom| self.heap.alloc(Cell::String(self.atom_name(atom).into())))
            .collect();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
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
                self.object_keys(args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectGetOwnPropertyNames => {
                self.object_names(args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectGetOwnPropertyDescriptor => {
                self.object_get_own_property_descriptor(p, args)
            }
            Native::ObjectGetOwnPropertyDescriptors => {
                self.object_get_own_property_descriptors(p, args)
            }
            Native::ObjectDefineProperty => self.object_define_property(p, args),
            Native::ObjectValues => {
                self.object_values(args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectEntries => {
                self.object_entries(args.first().copied().unwrap_or(Value::UNDEFINED))
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
            Native::ObjectAssign => self.object_assign(args),
            Native::ObjectGetPrototypeOf => {
                self.object_get_prototype_of(args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectSetPrototypeOf => self.object_set_prototype_of(
                args.first().copied().unwrap_or(Value::UNDEFINED),
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::ObjectHasOwn => {
                let target = args.first().copied().unwrap_or(Value::UNDEFINED);
                if target.is_null() || target.is_undefined() {
                    return Err(JsError("Object.hasOwn target is nullish".into()));
                }
                let target = self.box_object(target)?;
                let text = self.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                let key = self.intern_atom(&text);
                Ok(if self.own_property(target, key).is_some() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            _ => Err(JsError("invalid object native".into())),
        }
    }

    fn same_value(&self, left: Value, right: Value) -> bool {
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

    fn object_values(&mut self, object: Value) -> Result<Value, JsError> {
        let object = self.box_object(object)?;
        let data = self.object_data(object).expect("boxed target is object");
        let shape = self.ordered_shape(data);
        let values = shape
            .iter()
            .filter(|(atom, _)| self.is_enumerable(object, *atom))
            .filter_map(|(_, slot)| self.heap.property_get(data, *slot))
            .collect::<Vec<_>>();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    fn object_entries(&mut self, object: Value) -> Result<Value, JsError> {
        let object = self.box_object(object)?;
        let data = self.object_data(object).expect("boxed target is object");
        let shape = self.ordered_shape(data);
        let pairs = shape
            .iter()
            .filter(|(atom, _)| self.is_enumerable(object, *atom))
            .filter_map(|(atom, slot)| {
                self.heap
                    .property_get(data, *slot)
                    .map(|value| (*atom, value))
            })
            .collect::<Vec<_>>();
        let entries = pairs
            .into_iter()
            .map(|(atom, value)| {
                let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
                self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(vec![key, value]),
                })
            })
            .collect::<Vec<_>>();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(entries),
        }))
    }

    fn ordered_shape(&self, data: &Object) -> Vec<(Atom, usize)> {
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

    pub(super) fn object_names(&mut self, object: Value) -> Result<Value, JsError> {
        let object = self.box_object(object)?;
        let data = self.object_data(object).expect("boxed target is object");
        let values = self
            .ordered_shape(data)
            .into_iter()
            .map(|(atom, _)| self.heap.alloc(Cell::String(self.atom_name(atom).into())))
            .collect();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    pub(super) fn is_enumerable(&self, object: Value, atom: Atom) -> bool {
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

    pub(super) fn object_get_own_property_descriptor(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = self.box_object(args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let key = self.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let atom = self.intern_atom(&key);
        let Some(value) = self.own_property(target, atom) else {
            return Ok(Value::UNDEFINED);
        };
        let attributes = self
            .descriptors
            .get(&(target, atom))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        let descriptor = self.object();
        for (name, value) in [
            ("value", value),
            (
                "writable",
                if attributes.writable {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
            (
                "enumerable",
                if attributes.enumerable {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
            (
                "configurable",
                if attributes.configurable {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, value)?;
        }
        Ok(descriptor)
    }

    fn object_get_own_property_descriptors(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = self.box_object(args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let data = self.object_data(target).expect("boxed target is object");
        let keys = self.ordered_shape(data);
        let result = self.object();
        for (atom, _) in keys {
            let key = self.heap.alloc(Cell::String(self.atom_name(atom).into()));
            let descriptor = self.object_get_own_property_descriptor(p, &[target, key])?;
            self.set_property(result, atom, descriptor)?;
        }
        Ok(result)
    }

    pub(super) fn object_define_property(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        let target = self
            .object_data(target)
            .map(|_| target)
            .ok_or_else(|| JsError("defineProperty target is not an object".into()))?;
        let key = self.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let atom = self.intern_atom(&key);
        let descriptor = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        if self.object_data(descriptor).is_none() {
            return Err(JsError("property descriptor is not an object".into()));
        }
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

fn array_index(name: &str) -> Option<u32> {
    if name.is_empty() || name != "0" && name.starts_with('0') {
        return None;
    }
    let index = name.parse::<u32>().ok()?;
    (index.to_string() == name && index < u32::MAX).then_some(index)
}
