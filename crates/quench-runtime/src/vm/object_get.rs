use super::property_key::PropertyKey;
use super::*;
use crate::heap::PrivateBrand;

impl<H: Host> Vm<H> {
    pub(super) fn module_binding_value(&self, object: Value, atom: Atom) -> Option<Value> {
        let (program, slot) = self.object_data(object)?.module_binding(atom)?;
        let environment = self.programs.module_environment(program)?;
        self.heap.environment_slot(environment, slot as usize)
    }

    pub(super) fn module_namespace_value(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
    ) -> Result<Option<Value>, JsError> {
        let value = self
            .module_binding_value(object, atom)
            .or_else(|| self.own_property(object, atom));
        if value.is_some_and(Value::is_deleted) {
            return Err(self.reference_error(
                p,
                format!(
                    "Cannot access '{}' before initialization",
                    self.atom_name(atom)
                ),
            ));
        }
        Ok(value)
    }

    fn proxy_trap(
        &mut self,
        p: &ResidualProgram,
        handler: Value,
        name: &str,
    ) -> Result<Value, JsError> {
        let atom = self.intern_atom(name);
        self.get_property(p, handler, atom)
    }

    fn proxy_key_value(
        &mut self,
        p: &ResidualProgram,
        property: PropertyKey,
    ) -> Result<Value, JsError> {
        match property {
            PropertyKey::String(atom) if !self.is_private_name(atom) => {
                Ok(self.heap.alloc(Cell::String(self.atom_value(atom))))
            }
            PropertyKey::Symbol(key) => Ok(key),
            PropertyKey::String(_) | PropertyKey::Private(_) => {
                Err(self.type_error(p, "private member is not present on this object".into()))
            }
        }
    }

    pub(super) fn proxy_get(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        property: PropertyKey,
    ) -> Result<Value, JsError> {
        let key = self.proxy_key_value(p, property)?;
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        let target = self.heap.root(target);
        let handler = self.heap.root(handler);
        let receiver = self.heap.root(receiver);
        let key = self.heap.root(key);
        let mut result_root = None;
        let outcome = (|| {
            let trap = self.proxy_trap(p, self.heap.root_value(handler).unwrap(), "get")?;
            if trap.is_null() || trap.is_undefined() {
                let target = self.heap.root_value(target).unwrap();
                let receiver = self.heap.root_value(receiver).unwrap();
                return match property {
                    PropertyKey::String(atom) => {
                        self.get_property_with_receiver(p, target, atom, receiver)
                    }
                    PropertyKey::Symbol(_) => self.get_symbol_property_with_receiver(
                        p,
                        target,
                        self.heap.root_value(key).unwrap(),
                        receiver,
                    ),
                    PropertyKey::Private(_) => unreachable!("private keys cannot reach Proxy Get"),
                };
            }
            if !self.is_function(trap) {
                return Err(self.type_error(p, "proxy get trap is not callable".into()));
            }
            let result = self.call_value(
                p,
                trap,
                self.heap.root_value(handler).unwrap(),
                &[
                    self.heap.root_value(target).unwrap(),
                    self.heap.root_value(key).unwrap(),
                    self.heap.root_value(receiver).unwrap(),
                ],
            )?;
            let result = self.heap.root(result);
            result_root = Some(result);
            self.validate_proxy_get(p, target, key, result)?;
            Ok(self.heap.root_value(result).unwrap())
        })();
        for root in [
            Some(target),
            Some(handler),
            Some(receiver),
            Some(key),
            result_root,
        ]
        .into_iter()
        .flatten()
        {
            self.heap.release_root(root);
        }
        outcome
    }

    fn validate_proxy_get(
        &mut self,
        p: &ResidualProgram,
        target: RootId,
        key: RootId,
        result: RootId,
    ) -> Result<(), JsError> {
        let descriptor = self.object_get_own_property_descriptor(
            p,
            &[
                self.heap.root_value(target).unwrap(),
                self.heap.root_value(key).unwrap(),
            ],
        )?;
        if descriptor.is_undefined() {
            return Ok(());
        }
        let descriptor = self.own_descriptor_record(descriptor);
        if descriptor.configurable == Some(true) {
            return Ok(());
        }
        let result = self.heap.root_value(result).unwrap();
        if descriptor.has_data_fields()
            && descriptor.writable == Some(false)
            && !self.same_value(descriptor.value.unwrap_or(Value::UNDEFINED), result)
        {
            return Err(self.type_error(
                p,
                "proxy get trap returned a different value for a frozen property".into(),
            ));
        }
        if descriptor.has_accessor_fields()
            && descriptor.getter.is_none_or(Value::is_undefined)
            && !result.is_undefined()
        {
            return Err(self.type_error(
                p,
                "proxy get trap returned a value for an accessor without a getter".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn proxy_set(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        property: PropertyKey,
        value: Value,
    ) -> Result<bool, JsError> {
        let key = self.proxy_key_value(p, property)?;
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        let target = self.heap.root(target);
        let handler = self.heap.root(handler);
        let receiver = self.heap.root(receiver);
        let key = self.heap.root(key);
        let value = self.heap.root(value);
        let outcome = (|| {
            let trap = self.proxy_trap(p, self.heap.root_value(handler).unwrap(), "set")?;
            if trap.is_null() || trap.is_undefined() {
                let target = self.heap.root_value(target).unwrap();
                let receiver = self.heap.root_value(receiver).unwrap();
                let value = self.heap.root_value(value).unwrap();
                return match property {
                    PropertyKey::String(atom) => {
                        self.set_property_with_receiver(p, target, atom, value, receiver)
                    }
                    PropertyKey::Symbol(_) => self.set_symbol_property_with_receiver(
                        p,
                        target,
                        self.heap.root_value(key).unwrap(),
                        value,
                        receiver,
                    ),
                    PropertyKey::Private(_) => unreachable!("private keys cannot reach Proxy Set"),
                };
            }
            if !self.is_function(trap) {
                return Err(self.type_error(p, "proxy set trap is not callable".into()));
            }
            let result = self.call_value(
                p,
                trap,
                self.heap.root_value(handler).unwrap(),
                &[
                    self.heap.root_value(target).unwrap(),
                    self.heap.root_value(key).unwrap(),
                    self.heap.root_value(value).unwrap(),
                    self.heap.root_value(receiver).unwrap(),
                ],
            )?;
            if !self.truthy(result) {
                return Ok(false);
            }
            let descriptor = self.object_get_own_property_descriptor(
                p,
                &[
                    self.heap.root_value(target).unwrap(),
                    self.heap.root_value(key).unwrap(),
                ],
            )?;
            if !descriptor.is_undefined() && !self.descriptor_flag(descriptor, "configurable") {
                let value_atom = self.intern_atom("value");
                let writable_atom = self.intern_atom("writable");
                let setter_atom = self.intern_atom("set");
                let is_data = self.own_property(descriptor, value_atom).is_some()
                    || self.own_property(descriptor, writable_atom).is_some();
                if is_data
                    && !self.descriptor_flag(descriptor, "writable")
                    && self
                        .own_property(descriptor, value_atom)
                        .is_some_and(|target_value| {
                            !self.same_value(self.heap.root_value(value).unwrap(), target_value)
                        })
                {
                    return Err(self
                        .type_error(p, "proxy set trap changed a frozen target property".into()));
                }
                if !is_data
                    && self
                        .own_property(descriptor, setter_atom)
                        .is_none_or(Value::is_undefined)
                {
                    return Err(self.type_error(
                        p,
                        "proxy set trap accepted a property without a setter".into(),
                    ));
                }
            }
            Ok(true)
        })();
        for root in [target, handler, receiver, key, value] {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn get_symbol_property_with_receiver(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        receiver: Value,
    ) -> Result<Value, JsError> {
        let mut owner = self.primitive_prototype(object).unwrap_or(object);
        loop {
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = self.heap.get(owner)
            {
                let (target, handler) = (*target, *handler);
                return self.proxy_get(p, target, handler, receiver, PropertyKey::symbol(key));
            }
            if let Some(value) = self.symbol_property(owner, key) {
                let attributes = self
                    .property_attributes(owner, PropertyKey::symbol(key))
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                if attributes.accessor {
                    return attributes
                        .getter
                        .filter(|getter| !getter.is_undefined())
                        .map_or(Ok(Value::UNDEFINED), |getter| {
                            self.call_value(p, getter, receiver, &[])
                        });
                }
                return Ok(value);
            }
            let Some(data) = self.object_data(owner) else {
                return Ok(Value::UNDEFINED);
            };
            owner = data.proto;
            if owner.is_null() {
                return Ok(Value::UNDEFINED);
            }
        }
    }

    pub(super) fn get_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        self.get_property_with_receiver(p, object, atom, object)
    }

    pub(super) fn get_property_with_receiver(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        receiver: Value,
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        let private_name = self.is_private_name(atom);
        if object.is_null() || object.is_undefined() {
            return Err(self.type_error(
                p,
                if object.is_null() {
                    "cannot read properties of null".into()
                } else {
                    "cannot read properties of undefined".into()
                },
            ));
        }
        if !private_name
            && self
                .atom_class(atom)
                .contains(AtomClass::RESTRICTED_FUNCTION_PROPERTY)
            && matches!(self.heap.get(object), Some(Cell::Function { .. }))
            && self.own_property(object, atom).is_none()
            && !self.function_caller_is_restricted(object)
        {
            return match self.atom_name(atom) {
                "caller" => Ok(self.function_caller(object)),
                "arguments" => self.function_arguments(object),
                _ => unreachable!("legacy function reflection property"),
            };
        }
        let private_target = object;
        let mut object = object;
        if object.as_bool().is_some() {
            match self.atom_name(atom) {
                "toString" | "valueOf"
                    if self
                        .primitive_prototype(object)
                        .and_then(|prototype| self.own_property(prototype, atom))
                        .is_none() =>
                {
                    return Ok(self.native_value(if self.atom_name(atom) == "toString" {
                        Native::BooleanToString
                    } else {
                        Native::BooleanValueOf
                    }));
                }
                _ => {
                    object = self
                        .primitive_prototype(object)
                        .unwrap_or(self.object_proto)
                }
            }
        } else if object.as_number().is_some() {
            let prototype = self
                .primitive_prototype(object)
                .unwrap_or(self.object_proto);
            let method = self.own_property(prototype, atom);
            let builtin = if self.lookup_atom("toString") == Some(atom) {
                Some(Native::NumberString)
            } else if atom == self.to_fixed_atom {
                Some(Native::NumberFixed)
            } else if atom == self.to_precision_atom {
                Some(Native::NumberPrecision)
            } else {
                None
            };
            if method.is_none()
                && let Some(builtin) = builtin
            {
                return Ok(self.native_value(builtin));
            }
            object = prototype;
        }
        self.evaluate_deferred_namespace_for_key(
            p,
            object,
            Some(super::property_key::PropertyKey::string(atom)),
        )?;
        if private_name {
            self.check_private_brand(p, private_target, atom)?;
        }
        loop {
            self.evaluate_deferred_namespace_for_key(
                p,
                object,
                Some(super::property_key::PropertyKey::string(atom)),
            )?;
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = self.heap.get(object)
            {
                let (target, handler) = (*target, *handler);
                // Private names are not property keys observable through a
                // Proxy. Forwarding here would incorrectly let the target's
                // hidden storage satisfy a private access on the Proxy.
                if private_name {
                    return Err(
                        self.type_error(p, "private member is not present on this object".into())
                    );
                }
                return self.proxy_get(p, target, handler, receiver, PropertyKey::string(atom));
            }
            if let Some(attributes) = self.property_attributes(object, PropertyKey::string(atom))
                && attributes.accessor
            {
                if private_name && attributes.getter.is_none() {
                    return Err(
                        self.type_error(p, "private accessor does not have a getter".into())
                    );
                }
                return match attributes.getter {
                    Some(getter) => self.call_value(p, getter, receiver, &[]),
                    None => Ok(Value::UNDEFINED),
                };
            }
            if self
                .object_data(object)
                .is_some_and(Object::is_module_namespace)
            {
                return Ok(self
                    .module_namespace_value(p, object, atom)?
                    .unwrap_or(Value::UNDEFINED));
            }
            if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
                && let Some(index) = super::object_static::array_index(self.atom_name(atom))
                && let Some(Cell::Array { elements, .. }) = self.heap.get(object)
            {
                let value = elements
                    .get(index as usize)
                    .copied()
                    .filter(|value| !value.is_deleted())
                    .or_else(|| self.heap.sparse_get(object, index as usize));
                if let Some(value) = value.filter(|value| !value.is_deleted()) {
                    return Ok(value);
                }
            }
            if matches!(self.heap.get(object), Some(Cell::TypedArray { .. })) {
                match Self::typed_array_index_key(self.atom_name(atom)) {
                    super::object_descriptors::TypedArrayIndexKey::Index(index) => {
                        return Ok(self
                            .typed_array_get(object, index)
                            .unwrap_or(Value::UNDEFINED));
                    }
                    super::object_descriptors::TypedArrayIndexKey::Invalid => {
                        return Ok(Value::UNDEFINED);
                    }
                    super::object_descriptors::TypedArrayIndexKey::NotCanonical => {}
                }
            }
            if let Some(v) = self.own_property(object, atom) {
                return Ok(v);
            }
            match self.heap.get(object) {
                Some(Cell::ArrayBuffer { object: x, .. }) => object = x.proto,
                Some(Cell::TypedArray { object: x, .. }) => object = x.proto,
                Some(Cell::DataView { object: x, .. }) => object = x.proto,
                Some(Cell::String(v)) => {
                    if let Ok(index) = self.atom_name(atom).parse::<usize>()
                        && let Some(unit) = v.units().get(index).copied()
                    {
                        return Ok(self.heap.alloc(Cell::String(JsString::from_units(&[unit]))));
                    }
                    if atom == self.length_atom {
                        return Ok(Value::number(v.units().len() as f64));
                    }
                    let prototype = self
                        .primitive_prototype(object)
                        .unwrap_or(self.string_proto);
                    return self.get_property_with_receiver(p, prototype, atom, object);
                }
                Some(Cell::Symbol(description)) => {
                    let description = description.clone();
                    match self.atom_name(atom) {
                        "description" => {
                            return Ok(description
                                .as_ref()
                                .map(|value| self.heap.alloc(Cell::String(value.clone().into())))
                                .unwrap_or(Value::UNDEFINED));
                        }
                        "toString" => return Ok(self.native_value(Native::SymbolToString)),
                        "valueOf" => return Ok(self.native_value(Native::SymbolValueOf)),
                        _ => {
                            object = self
                                .primitive_prototype(object)
                                .unwrap_or(self.object_proto)
                        }
                    }
                }
                Some(Cell::BigInt(_)) => {
                    object = self
                        .primitive_prototype(object)
                        .unwrap_or(self.object_proto)
                }
                Some(Cell::Date { object: x, .. })
                | Some(Cell::TemporalDuration { object: x, .. })
                | Some(Cell::TemporalPlainDate { object: x, .. })
                | Some(Cell::TemporalPlainDateTime { object: x, .. })
                | Some(Cell::TemporalPlainMonthDay { object: x, .. })
                | Some(Cell::TemporalPlainYearMonth { object: x, .. })
                | Some(Cell::TemporalZonedDateTime { object: x, .. })
                | Some(Cell::TemporalInstant { object: x, .. }) => object = x.proto,
                Some(Cell::Object(x)) | Some(Cell::Array { object: x, .. }) => object = x.proto,
                Some(Cell::ShadowRealm { object: x, .. }) => object = x.proto,
                Some(Cell::RegExp { object: x, .. }) => object = x.proto,
                Some(Cell::Map { object: x, .. }) | Some(Cell::Set { object: x, .. }) => {
                    object = x.proto
                }
                Some(Cell::WeakMap { object: x, .. }) | Some(Cell::WeakSet { object: x, .. }) => {
                    object = x.proto
                }
                Some(Cell::WeakRef { object: x, .. }) => object = x.proto,
                Some(Cell::FinalizationRegistry { object: x, .. }) => object = x.proto,
                Some(Cell::Iterator { object: x, .. }) => object = x.proto,
                Some(Cell::Function { object: x, .. }) => object = x.proto,
                _ => return Ok(Value::UNDEFINED),
            }
            if object.is_null() {
                return if private_name {
                    let home = self.private_brand_home(p, private_target, atom);
                    if let Some(home) = home {
                        if let Some(attributes) = self.property_accessor(home, atom) {
                            if let Some(getter) = attributes.getter {
                                return self.call_value(p, getter, receiver, &[]);
                            }
                            return Err(self
                                .type_error(p, "private accessor does not have a getter".into()));
                        }
                        if let Some(value) = self.own_property(home, atom) {
                            return Ok(value);
                        }
                    }
                    Err(self.type_error(p, "private member is not present on this object".into()))
                } else {
                    Ok(Value::UNDEFINED)
                };
            }
        }
    }

    pub(super) fn check_private_brand(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Result<(), JsError> {
        if self.has_private_brand(p, target, atom) {
            return Ok(());
        }
        Err(self.type_error(p, "private member is not present on this object".into()))
    }

    pub(super) fn has_private_brand(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> bool {
        self.private_brand_home(p, target, atom).is_some()
    }

    pub(super) fn private_brand_home(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Option<Value> {
        let Some(frame_index) = self.frames.len().checked_sub(1) else {
            return None;
        };
        let private_home_binding = self.private_home_binding_atom(atom);
        let mut function = Some(self.frames[frame_index].function);
        let mut home_atoms = Vec::new();
        while let Some(id) = function
            && let Some(metadata) = p.functions.get(id as usize)
        {
            if let Some(atom) = metadata.super_home_atom
                && !home_atoms.contains(&atom)
            {
                home_atoms.push(atom);
            }
            function = metadata.parent;
        }
        let mut environment = self.captured_parent_environment(frame_index);
        let mut homes = Vec::with_capacity(home_atoms.len());
        while let Some(Cell::Environment {
            parent,
            function,
            slots,
            scope,
        }) = self.heap.get(environment)
        {
            let program = &scope.program;
            let dynamic_bindings = self.heap.environment_bindings(environment)?;
            if let Some((_, home)) = dynamic_bindings
                .iter()
                .rev()
                .find(|(binding, _)| *binding == private_home_binding)
            {
                return self
                    .object_data(target)
                    .is_some_and(|object| {
                        object.has_private_name(PrivateBrand {
                            home: *home,
                            name: atom,
                        })
                    })
                    .then_some(*home);
            }
            if *function != u32::MAX {
                let owner = program.and_then(|program| {
                    self.programs
                        .get(super::program_store::ProgramId::from_raw(program))
                });
                let owner = owner.as_deref().unwrap_or(p);
                let visible_homes = owner
                    .functions
                    .get(*function as usize)
                    .into_iter()
                    .flat_map(|metadata| metadata.local_atoms.iter().copied())
                    .filter(|atom| self.atom_name(*atom).starts_with("\0quench:home:"))
                    .collect::<Vec<_>>();
                for atom in home_atoms.iter().chain(&visible_homes) {
                    if homes.iter().any(|(candidate, _)| candidate == atom) {
                        continue;
                    }
                    if let Some(slot) = self
                        .local_binding_slot(owner, *function, *atom)
                        .filter(|slot| *slot < slots.len())
                    {
                        homes.push((*atom, self.heap.environment_slot(environment, slot)?));
                    }
                }
            }
            environment = *parent;
        }
        for (_, home) in homes.iter().copied() {
            let declares_name = self
                .object_data(home)
                .is_some_and(|object| object.has_private_name(PrivateBrand { home, name: atom }));
            if !declares_name {
                continue;
            }
            let branded = self
                .object_data(target)
                .is_some_and(|object| object.has_private_name(PrivateBrand { home, name: atom }));
            return branded.then_some(home);
        }
        None
    }

    pub(super) fn get_private_proxy_field(
        &mut self,
        p: &ResidualProgram,
        proxy: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        let Some(home) = self.private_brand_home(p, proxy, atom) else {
            return Err(self.type_error(p, "private member is not present on this object".into()));
        };
        if let Some(value) = self.own_property(proxy, atom) {
            return Ok(value);
        }
        if let Some(attributes) = self.property_accessor(home, atom) {
            let Some(getter) = attributes.getter else {
                return Err(
                    self.type_error(p, "private member is not present on this object".into())
                );
            };
            return self.call_value(p, getter, proxy, &[]);
        }
        self.get_property(p, home, atom)
    }
}
