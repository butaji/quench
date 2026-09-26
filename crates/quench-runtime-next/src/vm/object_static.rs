use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn object_prototype_define_accessor(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = self.box_object_or_type_error(p, receiver)?;
        let accessor = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if !accessor.is_undefined() && self.call_target(accessor).is_err() {
            return Err(self.type_error(p, "accessor is not callable".into()));
        }
        let key = self.to_property_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let descriptor = self.object();
        let field = if native == Native::ObjectPrototypeDefineGetter {
            "get"
        } else {
            "set"
        };
        let field = self.intern_atom(field);
        let enumerable = self.intern_atom("enumerable");
        let configurable = self.intern_atom("configurable");
        self.set_property(descriptor, field, accessor)?;
        self.set_property(descriptor, enumerable, Value::TRUE)?;
        self.set_property(descriptor, configurable, Value::TRUE)?;
        self.object_define_property(p, &[target, key, descriptor])?;
        Ok(Value::UNDEFINED)
    }

    pub(super) fn define_class_field(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: PropertyKey,
        value: Value,
    ) -> Result<(), JsError> {
        if let PropertyKey::String(atom) = key
            && let Some(Cell::Object(object)) = self.heap.get(target)
            && !object.module_namespace
        {
            let exists = self.own_property(target, atom).is_some();
            if exists
                && self
                    .property_attributes(target, key)
                    .is_some_and(|attributes| !attributes.configurable)
            {
                return Err(self.type_error(p, "cannot redefine non-configurable property".into()));
            }
            if !exists && !object.is_extensible() {
                return Err(
                    self.type_error(p, "cannot add property to non-extensible object".into())
                );
            }
            if exists {
                self.remove_property_attributes(target, key);
            }
            self.set_shape_property(target, key, value)?;
            self.set_property_attributes(target, key, DEFAULT_PROPERTY_ATTRIBUTES);
            return Ok(());
        }

        let key_value = match key {
            PropertyKey::String(atom) => {
                let text = self.atom_name(atom).to_owned();
                self.heap.alloc(Cell::String(text.into()))
            }
            PropertyKey::Symbol(symbol) => symbol,
            PropertyKey::Private(_) => {
                return Err(JsError::validation(
                    "private names are not public class-field keys".into(),
                ));
            }
        };
        let descriptor = self.object();
        for (name, field) in [
            ("value", value),
            ("writable", Value::TRUE),
            ("enumerable", Value::TRUE),
            ("configurable", Value::TRUE),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, field)?;
        }
        self.object_define_property(p, &[target, key_value, descriptor])?;
        Ok(())
    }

    pub(super) fn object_prototype_to_locale_string(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        self.require_object_coercible(p, this)?;
        if this.as_number().is_some() || matches!(self.heap.get(this), Some(Cell::BigInt(_))) {
            let text = self.to_string(p, this)?;
            return Ok(self.heap.alloc(Cell::String(text.into())));
        }
        for marker in ["\0rqj:number-value", "\0rqj:bigint-value"] {
            if let Some(value) = self
                .lookup_atom(marker)
                .and_then(|atom| self.own_property(this, atom))
            {
                let text = self.to_string(p, value)?;
                return Ok(self.heap.alloc(Cell::String(text.into())));
            }
        }
        let to_string = self.intern_atom("toString");
        let method = self.get_property(p, this, to_string)?;
        if !self.is_function(method) {
            return Err(self.type_error(p, "toString is not callable".into()));
        }
        self.call_value(p, method, this, &[])
    }

    pub(super) fn object_prototype_to_string(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let proxy_array =
            matches!(self.heap.get(value), Some(Cell::Proxy { .. })) && self.is_array(p, value)?;
        let boxed_brand = [
            ("\0rqj:string-value", "String"),
            ("\0rqj:boolean-value", "Boolean"),
            ("\0rqj:number-value", "Number"),
        ]
        .into_iter()
        .find_map(|(marker, brand)| {
            self.lookup_atom(marker)
                .and_then(|atom| self.own_property(value, atom))
                .map(|_| brand)
        });
        let brand = if value.is_undefined() || value.is_deleted() {
            "Undefined"
        } else if value.is_null() {
            "Null"
        } else if value.as_bool().is_some() {
            "Boolean"
        } else if value.as_number().is_some() {
            "Number"
        } else if self
            .lookup_atom("\0rqj:error-brand")
            .and_then(|atom| self.own_property(value, atom))
            .is_some_and(|brand| brand == Value::TRUE)
        {
            "Error"
        } else if let Some(brand) = boxed_brand {
            brand
        } else {
            match self.heap.get(value) {
                Some(Cell::Proxy { .. }) if proxy_array => "Array",
                Some(Cell::Proxy { .. }) if self.is_function(value) => "Function",
                Some(Cell::String(_)) => "String",
                Some(Cell::BigInt(_) | Cell::Symbol(_)) => "Object",
                Some(Cell::Array { .. })
                    if self
                        .object_data(value)
                        .is_some_and(Object::is_arguments_object) =>
                {
                    "Arguments"
                }
                Some(Cell::Array { .. }) => "Array",
                Some(Cell::Date { .. }) => "Date",
                Some(Cell::RegExp { .. }) => "RegExp",
                Some(Cell::Function { .. }) => "Function",
                Some(Cell::ArrayBuffer { .. }) => "ArrayBuffer",
                Some(Cell::DataView { .. }) => "DataView",
                Some(Cell::TypedArray { kind, .. }) => match kind {
                    TypedArrayKind::Uint8 => "Uint8Array",
                    TypedArrayKind::Uint8Clamped => "Uint8ClampedArray",
                    TypedArrayKind::Uint16 => "Uint16Array",
                    TypedArrayKind::Uint32 => "Uint32Array",
                    TypedArrayKind::Int8 => "Int8Array",
                    TypedArrayKind::Int16 => "Int16Array",
                    TypedArrayKind::Int32 => "Int32Array",
                    TypedArrayKind::BigInt64 => "BigInt64Array",
                    TypedArrayKind::BigUint64 => "BigUint64Array",
                    TypedArrayKind::Float32 => "Float32Array",
                    TypedArrayKind::Float64 => "Float64Array",
                },
                Some(Cell::WeakRef { .. }) => "WeakRef",
                Some(Cell::FinalizationRegistry { .. }) => "FinalizationRegistry",
                Some(Cell::Error(_)) => "Error",
                _ => "Object",
            }
        };
        let tag = if value.is_null() || value.is_undefined() {
            None
        } else {
            self.well_known_symbols
                .get("toStringTag")
                .copied()
                .map(|symbol| self.get_index(p, value, symbol))
                .transpose()?
        };
        let brand = tag
            .and_then(|tag| match self.heap.get(tag) {
                Some(Cell::String(text)) => Some(text.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| brand.to_owned());
        Ok(self
            .heap
            .alloc(Cell::String(format!("[object {brand}]").into())))
    }

    pub(super) fn descriptor_field(
        &mut self,
        p: &ResidualProgram,
        descriptor: Value,
        name: &str,
    ) -> Result<Option<Value>, JsError> {
        let atom = self.intern_atom(name);
        let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
        if !self.has_property(p, descriptor, key)? {
            return Ok(None);
        }
        Ok(Some(self.get_property(p, descriptor, atom)?))
    }

    pub(super) fn object_assign(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target =
            self.box_object_or_type_error(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        for source in args.iter().copied().skip(1) {
            if source.is_null() || source.is_undefined() {
                continue;
            }
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
                        self.set_index_mode(p, target, key, value, true)?;
                    }
                    Some(Cell::String(name)) => {
                        let atom = self.intern_js_atom(&name);
                        let value = self.get_property(p, source, atom)?;
                        self.set_property_with_program_mode(p, target, atom, value, true)?;
                    }
                    _ => unreachable!("validated own property key"),
                }
            }
        }
        Ok(target)
    }

    pub(super) fn object_for_in_keys(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Value, JsError> {
        if source.is_null() || source.is_undefined() {
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(Vec::new()),
            }));
        }
        let mut current = self.box_object(source)?;
        let mut visited_objects = std::collections::HashSet::new();
        let mut visited_names = std::collections::HashSet::new();
        let mut keys = Vec::new();
        while !current.is_null() && visited_objects.insert(current) {
            for key in self.object_own_key_values(p, current)? {
                let Some(Cell::String(name)) = self.heap.get(key) else {
                    continue;
                };
                let name = name.clone();
                if visited_names.contains(&name) {
                    continue;
                }
                let descriptor = self.object_get_own_property_descriptor(p, &[current, key])?;
                if descriptor.is_undefined() {
                    continue;
                }
                visited_names.insert(name);
                if self.descriptor_flag(descriptor, "enumerable") {
                    keys.push(key);
                }
            }
            current = self.object_get_prototype_of(p, current)?;
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(keys),
        }))
    }

    pub(super) fn object_for_in_key_is_enumerable(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        key: Value,
    ) -> Result<bool, JsError> {
        if source.is_null() || source.is_undefined() {
            return Ok(false);
        }
        let mut current = self.box_object(source)?;
        let mut visited_objects = std::collections::HashSet::new();
        while !current.is_null() && visited_objects.insert(current) {
            let descriptor = self.object_get_own_property_descriptor(p, &[current, key])?;
            if !descriptor.is_undefined() {
                return Ok(self.descriptor_flag(descriptor, "enumerable"));
            }
            current = self.object_get_prototype_of(p, current)?;
        }
        Ok(false)
    }

    pub(super) fn install_object_extra(
        &mut self,
        program: &ResidualProgram,
        object: Value,
    ) -> Result<(), JsError> {
        for (name, native) in [
            ("toLocaleString", Native::ObjectPrototypeToLocaleString),
            ("toString", Native::ObjectPrototypeToString),
            ("valueOf", Native::ObjectPrototypeValueOf),
            ("hasOwnProperty", Native::ObjectPrototypeHasOwnProperty),
            (
                "propertyIsEnumerable",
                Native::ObjectPrototypePropertyIsEnumerable,
            ),
            ("__lookupGetter__", Native::ObjectPrototypeLookupGetter),
            ("__lookupSetter__", Native::ObjectPrototypeLookupSetter),
            ("__defineGetter__", Native::ObjectPrototypeDefineGetter),
            ("__defineSetter__", Native::ObjectPrototypeDefineSetter),
            ("isPrototypeOf", Native::ObjectPrototypeIsPrototypeOf),
        ] {
            self.set_builtin_named(program, self.object_proto, name, native)?;
        }
        let prototype_atom = self.intern_atom("__proto__");
        let getter = self.native_value(Native::ObjectPrototypeProtoGetter);
        let setter = self.native_value(Native::ObjectPrototypeProtoSetter);
        self.set_builtin_function_name(getter, "get __proto__")?;
        self.set_builtin_function_name(setter, "set __proto__")?;
        self.set_property(self.object_proto, prototype_atom, Value::UNDEFINED)?;
        self.set_property_attributes(
            self.object_proto,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(getter),
                setter: Some(setter),
            },
        );
        for (name, native) in [
            ("getOwnPropertyNames", Native::ObjectGetOwnPropertyNames),
            (
                "getOwnPropertyDescriptor",
                Native::ObjectGetOwnPropertyDescriptor,
            ),
            ("getOwnPropertySymbols", Native::ObjectGetOwnPropertySymbols),
            (
                "getOwnPropertyDescriptors",
                Native::ObjectGetOwnPropertyDescriptors,
            ),
            ("defineProperty", Native::ObjectDefineProperty),
            ("defineProperties", Native::ObjectDefineProperties),
            ("values", Native::ObjectValues),
            ("entries", Native::ObjectEntries),
            ("fromEntries", Native::ObjectFromEntries),
            ("is", Native::ObjectIs),
        ] {
            self.set_builtin_named(program, object, name, native)?;
        }
        Ok(())
    }

    pub(super) fn call_object_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::Object => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                if value.is_null() || value.is_undefined() {
                    Ok(self.object())
                } else if matches!(self.heap.get(value), Some(Cell::BigInt(_))) {
                    self.box_bigint_object(p, value)
                } else {
                    self.box_object(value)
                }
            }
            Native::ObjectKeys => {
                self.object_keys(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ForInKeys => {
                self.object_for_in_keys(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ForInKeyIsEnumerable => Ok(
                if self.object_for_in_key_is_enumerable(
                    p,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                    args.get(1).copied().unwrap_or(Value::UNDEFINED),
                )? {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
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
            Native::ObjectDefineProperties => self.object_define_properties(p, args),
            Native::ObjectValues => {
                self.object_values(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectEntries => {
                self.object_entries(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectFromEntries => {
                self.object_from_entries(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ObjectGroupBy => self.object_group_by(p, args),
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
                let object = self.heap.alloc(Cell::Object(Self::empty_object(proto)));
                if let Some(descriptors) = args.get(1).copied()
                    && !descriptors.is_undefined()
                {
                    self.object_define_properties(p, &[object, descriptors])?;
                }
                Ok(object)
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
                    return Err(self.type_error(p, "Object.hasOwn target is nullish".into()));
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
            Native::ObjectIsSealed => self.object_is_integrity_level(p, args, false),
            Native::ObjectFreeze => self.object_set_integrity(p, args, true),
            Native::ObjectIsFrozen => self.object_is_integrity_level(p, args, true),
            _ => Err(JsError("invalid object native".into())),
        }
    }

    fn object_group_by(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let iterable = args.first().copied().unwrap_or(Value::UNDEFINED);
        let callback = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if self.call_target(callback).is_err() {
            return Err(self.type_error(p, "Object.groupBy callback is not callable".into()));
        }
        let iterator = self.get_iterator(p, iterable)?;
        let result = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        let roots = [self.heap.root(iterator), self.heap.root(result)];
        let outcome = (|| {
            let done_atom = self.intern_atom("done");
            let value_atom = self.intern_atom("value");
            let mut index = 0usize;
            loop {
                let iterator = self.heap.root_value(roots[0]).unwrap_or(iterator);
                let result = self.heap.root_value(roots[1]).unwrap_or(result);
                let step = match self.iterator_next(p, iterator) {
                    Ok(step) => step,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                let step_root = self.heap.root(step);
                let step = self.heap.root_value(step_root).unwrap_or(step);
                let done = match self.get_property(p, step, done_atom) {
                    Ok(done) => done,
                    Err(error) => {
                        self.heap.release_root(step_root);
                        return Err(self.iterator_abrupt(p, iterator, error));
                    }
                };
                if self.truthy(done) {
                    self.heap.release_root(step_root);
                    return Ok(result);
                }
                let value = match self.get_property(p, step, value_atom) {
                    Ok(value) => value,
                    Err(error) => {
                        self.heap.release_root(step_root);
                        return Err(self.iterator_abrupt(p, iterator, error));
                    }
                };
                let value_root = self.heap.root(value);
                let value = self.heap.root_value(value_root).unwrap_or(value);
                let key = match self.call_value(
                    p,
                    callback,
                    Value::UNDEFINED,
                    &[value, Value::number(index as f64)],
                ) {
                    Ok(key) => key,
                    Err(error) => {
                        self.heap.release_root(value_root);
                        self.heap.release_root(step_root);
                        return Err(self.iterator_abrupt(p, iterator, error));
                    }
                };
                let key = match self.to_property_key(p, key) {
                    Ok(key) => key,
                    Err(error) => {
                        self.heap.release_root(value_root);
                        self.heap.release_root(step_root);
                        return Err(self.iterator_abrupt(p, iterator, error));
                    }
                };
                let group = match self.heap.get(key).cloned() {
                    Some(Cell::Symbol(_)) => self.symbol_property(result, key),
                    Some(Cell::String(name)) => {
                        let atom = self.intern_js_atom(&name);
                        self.own_property(result, atom)
                    }
                    _ => None,
                };
                if let Some(group) = group {
                    if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(group) {
                        Rc::make_mut(elements).push(value);
                    }
                } else {
                    let group = self.heap.alloc(Cell::Array {
                        object: Self::empty_object(self.array_proto),
                        elements: Rc::new(vec![value]),
                    });
                    let set_result = match self.heap.get(key).cloned() {
                        Some(Cell::Symbol(_)) => self.set_index(p, result, key, group),
                        Some(Cell::String(name)) => {
                            let atom = self.intern_js_atom(&name);
                            self.set_property(result, atom, group)
                        }
                        _ => unreachable!("ToPropertyKey returns a string or symbol"),
                    };
                    if let Err(error) = set_result {
                        self.heap.release_root(value_root);
                        self.heap.release_root(step_root);
                        return Err(self.iterator_abrupt(p, iterator, error));
                    }
                }
                self.heap.release_root(value_root);
                self.heap.release_root(step_root);
                index += 1;
            }
        })();
        self.heap.release_root(roots[0]);
        self.heap.release_root(roots[1]);
        outcome
    }

    fn object_from_entries(
        &mut self,
        p: &ResidualProgram,
        iterable: Value,
    ) -> Result<Value, JsError> {
        let iterator = self.get_iterator(p, iterable)?;
        let result = self.object();
        let mut roots = vec![self.heap.root(iterator), self.heap.root(result)];
        let outcome = (|| {
            let done_atom = self.intern_atom("done");
            let value_atom = self.intern_atom("value");
            let mut object = result;
            loop {
                let iteration_roots = roots.len();
                let iterator = self.heap.root_value(roots[0]).unwrap_or(iterator);
                let step = match self.iterator_next(p, iterator) {
                    Ok(step) => step,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                roots.push(self.heap.root(step));
                let step = self.heap.root_value(*roots.last().unwrap()).unwrap_or(step);
                let done = match self.get_property(p, step, done_atom) {
                    Ok(done) => done,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                if self.truthy(done) {
                    for root in roots.drain(iteration_roots..) {
                        self.heap.release_root(root);
                    }
                    break;
                }
                let entry = match self.get_property(p, step, value_atom) {
                    Ok(entry) => entry,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                if !self.is_object_like(entry) {
                    let error =
                        self.type_error(p, "Object.fromEntries entry is not an object".into());
                    return Err(self.iterator_abrupt(p, iterator, error));
                }
                roots.push(self.heap.root(entry));
                let entry = self
                    .heap
                    .root_value(*roots.last().unwrap())
                    .unwrap_or(entry);
                let raw_key = match self.get_index(p, entry, Value::number(0.0)) {
                    Ok(key) => key,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                roots.push(self.heap.root(raw_key));
                let raw_key = self
                    .heap
                    .root_value(*roots.last().unwrap())
                    .unwrap_or(raw_key);
                let value = match self.get_index(p, entry, Value::number(1.0)) {
                    Ok(value) => value,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                roots.push(self.heap.root(value));
                let value = self
                    .heap
                    .root_value(*roots.last().unwrap())
                    .unwrap_or(value);
                let key = match self.to_property_key(p, raw_key) {
                    Ok(key) => key,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                roots.push(self.heap.root(key));
                let key = self.heap.root_value(*roots.last().unwrap()).unwrap_or(key);
                let descriptor = self.object();
                roots.push(self.heap.root(descriptor));
                for (name, field) in [
                    ("value", value),
                    ("writable", Value::TRUE),
                    ("enumerable", Value::TRUE),
                    ("configurable", Value::TRUE),
                ] {
                    let atom = self.intern_atom(name);
                    if let Err(error) = self.set_property(descriptor, atom, field) {
                        return Err(self.iterator_abrupt(p, iterator, error));
                    }
                }
                object = self.heap.root_value(roots[1]).unwrap_or(object);
                if let Err(error) = self.object_define_property(p, &[object, key, descriptor]) {
                    return Err(self.iterator_abrupt(p, iterator, error));
                }
                for root in roots.drain(iteration_roots..) {
                    self.heap.release_root(root);
                }
            }
            Ok(self.heap.root_value(roots[1]).unwrap_or(object))
        })();
        roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        outcome
    }

    pub(super) fn same_value(&self, left: Value, right: Value) -> bool {
        if let (Some(a), Some(b)) = (left.as_number(), right.as_number()) {
            return (a.is_nan() && b.is_nan())
                || (a == b && (a != 0.0 || a.is_sign_negative() == b.is_sign_negative()));
        }
        match (self.heap.get(left), self.heap.get(right)) {
            (Some(Cell::String(a)), Some(Cell::String(b))) => a == b,
            (Some(Cell::BigInt(a)), Some(Cell::BigInt(b))) => a == b,
            _ => left == right,
        }
    }

    pub(super) fn ordered_shape(&self, data: &Object) -> Vec<(Atom, usize)> {
        let mut entries = self.shapes[data.shape() as usize]
            .keys
            .iter()
            .copied()
            .filter_map(|key| match key {
                PropertyKey::String(atom) => self.shapes[data.shape() as usize]
                    .slots
                    .get(&key)
                    .copied()
                    .map(|slot| (atom, slot as usize)),
                PropertyKey::Symbol(_) | PropertyKey::Private(_) => None,
            })
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

    pub(super) fn box_object(&mut self, value: Value) -> Result<Value, JsError> {
        if self.object_data(value).is_some() {
            return Ok(value);
        }
        if value.is_null() || value.is_undefined() {
            return Err(JsError("cannot convert nullish value to object".into()));
        }
        self.box_primitive_object(value)
    }

    pub(super) fn box_object_or_type_error(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        self.require_object_coercible(p, value)?;
        self.box_object(value)
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
                return Err(self.type_error(p, "cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("defineProperty");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let key = if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
                    key_value
                } else {
                    let text = self.coerce_js_string(p, key_value)?;
                    self.heap.alloc(Cell::String(text))
                };
                let descriptor = args.get(2).copied().unwrap_or(Value::UNDEFINED);
                if self.object_data(descriptor).is_none() {
                    return Err(self.type_error(p, "property descriptor is not an object".into()));
                }
                let result = self.call_value(p, trap, handler, &[target, key, descriptor])?;
                if !self.truthy(result) {
                    return Err(
                        self.type_error(p, "proxy defineProperty trap returned false".into())
                    );
                }
                self.validate_proxy_define_property(p, target, key, descriptor)?;
                return Ok(source);
            }
        }
        let target = self.proxy_target(source);
        let target = self
            .object_data(target)
            .map(|_| target)
            .ok_or_else(|| self.type_error(p, "defineProperty target is not an object".into()))?;
        let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let descriptor = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        if self.object_data(descriptor).is_none() {
            return Err(self.type_error(p, "property descriptor is not an object".into()));
        }
        if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
            return self.define_ordinary_property_key(
                p,
                target,
                PropertyKey::symbol(key_value),
                descriptor,
            );
        }
        let key = self.coerce_js_string(p, key_value)?;
        if matches!(self.heap.get(target), Some(Cell::TypedArray { .. }))
            && let Some(index) = Self::canonical_typed_array_index(key.host_string())
        {
            return self.define_typed_array_property(p, target, index, descriptor);
        }
        if key.host_string() == "length"
            && matches!(self.heap.get(target), Some(Cell::Array { .. }))
            && !self
                .object_data(target)
                .is_some_and(Object::is_arguments_object)
        {
            return if self.define_array_length(p, target, descriptor)? {
                Ok(target)
            } else {
                Err(self.type_error(p, "cannot redefine array length".into()))
            };
        }
        if let Some(index) = array_index(key.host_string()).map(|index| index as usize)
            && matches!(self.heap.get(target), Some(Cell::Array { .. }))
        {
            return self.define_array_property(p, target, index, descriptor);
        }
        let atom = self.intern_js_atom(&key);
        self.evaluate_deferred_namespace_for_key(
            p,
            target,
            Some(crate::vm::property_key::PropertyKey::string(atom)),
        )?;
        if self
            .object_data(target)
            .is_some_and(Object::is_module_namespace)
        {
            let Some(current) = self.own_property(target, atom) else {
                return Err(
                    self.type_error(p, "cannot define a non-export on a module namespace".into())
                );
            };
            let configurable = self.descriptor_field(p, descriptor, "configurable")?;
            let enumerable = self.descriptor_field(p, descriptor, "enumerable")?;
            let writable = self.descriptor_field(p, descriptor, "writable")?;
            let value = self.descriptor_field(p, descriptor, "value")?;
            let getter = self.descriptor_field(p, descriptor, "get")?;
            let setter = self.descriptor_field(p, descriptor, "set")?;
            let compatible = !configurable.is_some_and(|value| self.truthy(value))
                && !enumerable.is_some_and(|value| !self.truthy(value))
                && !writable.is_some_and(|value| !self.truthy(value))
                && getter.is_none()
                && setter.is_none()
                && value.is_none_or(|value| self.same_value(current, value));
            return if compatible {
                Ok(target)
            } else {
                Err(self.type_error(p, "cannot redefine module namespace export".into()))
            };
        }
        self.define_ordinary_property_key(p, target, PropertyKey::string(atom), descriptor)
    }

    fn define_ordinary_property_key(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: PropertyKey,
        descriptor: Value,
    ) -> Result<Value, JsError> {
        let existing = match key {
            PropertyKey::String(atom) => self.own_property(target, atom),
            PropertyKey::Symbol(symbol) => self.symbol_property(target, symbol),
            PropertyKey::Private(_) => None,
        };
        let current = self
            .property_attributes(target, key)
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
            if let Some(value) = self.descriptor_field(p, descriptor, name)? {
                *slot = self.truthy(value);
            }
        }
        if !is_new
            && !current.configurable
            && (attributes.configurable != current.configurable
                || attributes.enumerable != current.enumerable
                || attributes.writable && !current.writable)
        {
            return Err(self.type_error(p, "cannot redefine non-configurable property".into()));
        }
        let descriptor_value = self.descriptor_field(p, descriptor, "value")?;
        let descriptor_getter = self.descriptor_field(p, descriptor, "get")?;
        let descriptor_setter = self.descriptor_field(p, descriptor, "set")?;
        let descriptor_writable = self.descriptor_field(p, descriptor, "writable")?;
        let descriptor_accessor = descriptor_getter.is_some() || descriptor_setter.is_some();
        let descriptor_data = descriptor_value.is_some() || descriptor_writable.is_some();
        if descriptor_accessor && descriptor_data {
            return Err(self.type_error(
                p,
                "property descriptor mixes data and accessor fields".into(),
            ));
        }
        if !is_new
            && !current.configurable
            && descriptor_accessor != current.accessor
            && (descriptor_accessor || descriptor_data)
        {
            return Err(self.type_error(p, "cannot change non-configurable property kind".into()));
        }
        let accessor = descriptor_accessor;
        if accessor {
            let getter = descriptor_getter
                .map(|value| (!value.is_undefined()).then_some(value))
                .or_else(|| current.accessor.then_some(current.getter))
                .flatten();
            let setter = descriptor_setter
                .map(|value| (!value.is_undefined()).then_some(value))
                .or_else(|| current.accessor.then_some(current.setter))
                .flatten();
            if !is_new
                && current.accessor
                && !current.configurable
                && ((descriptor_getter.is_some()
                    && !descriptor_getter.is_some_and(|value| {
                        (value.is_undefined() && current.getter.is_none())
                            || current
                                .getter
                                .is_some_and(|old| self.same_value(old, value))
                    }))
                    || (descriptor_setter.is_some()
                        && !descriptor_setter.is_some_and(|value| {
                            (value.is_undefined() && current.setter.is_none())
                                || current
                                    .setter
                                    .is_some_and(|old| self.same_value(old, value))
                        })))
            {
                return Err(self.type_error(p, "cannot change non-configurable accessor".into()));
            }
            if getter.is_some_and(|value| !self.is_function(value))
                || setter.is_some_and(|value| !self.is_function(value))
            {
                return Err(self.type_error(p, "property accessor is not callable".into()));
            }
            if !is_new && !current.configurable && !current.accessor {
                return Err(self.type_error(p, "cannot redefine non-configurable property".into()));
            }
            if is_new {
                self.set_shape_property(target, key, Value::UNDEFINED)?;
            }
            self.set_property_attributes(
                target,
                key,
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
            && descriptor_value.is_some_and(|next| !self.same_value(existing.unwrap(), next))
        {
            return Err(self.type_error(p, "cannot write non-writable property".into()));
        }
        let value = descriptor_value.or(existing).unwrap_or(Value::UNDEFINED);
        if descriptor_data {
            attributes.accessor = false;
            attributes.getter = None;
            attributes.setter = None;
        }
        if is_new || descriptor_value.is_some() && (current.writable || current.configurable) {
            if (current.accessor || !current.writable && current.configurable) && descriptor_data {
                self.remove_property_attributes(target, key);
            }
            self.set_shape_property(target, key, value)?;
        }
        self.set_property_attributes(target, key, attributes);
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
