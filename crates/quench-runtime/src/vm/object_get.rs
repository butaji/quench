use super::property_key::PropertyKey;
use super::*;
use crate::heap::PrivateBrand;

pub(super) enum DiagnosticProperty {
    Absent,
    Data(Value),
    Blocked,
}

impl<H: Host> Vm<H> {
    /// `constructor[@@species]`. Every intrinsic `get [Symbol.species]` returns its receiver
    /// and observes nothing, so an own accessor that still holds one answers without a call.
    pub(super) fn species_of(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
    ) -> Result<Value, JsError> {
        let Some(species) = self.well_known_symbols.get("species").copied() else {
            return Ok(Value::UNDEFINED);
        };
        let intrinsic_getter = self
            .property_attributes(constructor, PropertyKey::symbol(species))
            .and_then(|attributes| attributes.getter.filter(|_| attributes.accessor))
            .is_some_and(|getter| {
                matches!(
                    self.heap.get(getter),
                    Some(Cell::Function {
                        kind: FunctionKind::Native(native),
                        ..
                    }) if native.is_intrinsic_species_getter()
                )
            });
        if intrinsic_getter {
            Ok(constructor)
        } else {
            self.get_index(p, constructor, species)
        }
    }

    pub(super) fn diagnostic_own_property(
        &self,
        object: Value,
        key: PropertyKey,
    ) -> DiagnosticProperty {
        if self.default_function_name_info(object, key)
            || self.proxy_parts(object).is_some()
            || self
                .property_attributes(object, key)
                .is_some_and(|attributes| attributes.accessor)
        {
            return DiagnosticProperty::Blocked;
        }
        let value = match key {
            PropertyKey::String(atom) => self.own_property(object, atom),
            PropertyKey::Symbol(symbol) => self.symbol_property(object, symbol),
            PropertyKey::Private(_) => return DiagnosticProperty::Blocked,
        };
        value.map_or(DiagnosticProperty::Absent, DiagnosticProperty::Data)
    }

    /// Inspect data properties for diagnostics without invoking accessors or Proxy traps.
    pub(super) fn diagnostic_data_property(
        &self,
        mut object: Value,
        key: PropertyKey,
    ) -> Option<Value> {
        loop {
            let data = self.object_data(object)?;
            match self.diagnostic_own_property(object, key) {
                DiagnosticProperty::Data(value) => return Some(value),
                DiagnosticProperty::Blocked => return None,
                DiagnosticProperty::Absent => {}
            }
            object = data.proto();
            if object.is_null() {
                return None;
            }
        }
    }

    pub(super) fn nullish_property_error(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        property: Option<&str>,
    ) -> JsError {
        let kind = if object.is_null() {
            "null"
        } else {
            "undefined"
        };
        let message = match property {
            Some(property) => format!("Cannot read properties of {kind} (reading '{property}')"),
            None => format!("Cannot read properties of {kind}"),
        };
        self.type_error(p, message)
    }

    pub(super) fn module_binding_value(&self, object: Value, atom: Atom) -> Option<Value> {
        let (program, slot) = self.object_data(object)?.module_binding(atom)?;
        if let Some(value) =
            self.module_import_value(program, super::ROOT_FUNCTION_ID, usize::from(slot))
        {
            return Some(value);
        }
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
        self.proxy_set_mode(p, target, handler, receiver, property, value, false)
    }

    pub(super) fn proxy_set_mode(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        property: PropertyKey,
        value: Value,
        strict: bool,
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
                let delegate_target = self.heap.root_value(target).unwrap();
                let receiver = self.heap.root_value(receiver).unwrap();
                let value = self.heap.root_value(value).unwrap();
                if let Some((inner, handler)) = self.proxy_parts(delegate_target) {
                    return self
                        .proxy_set_mode(p, inner, handler, receiver, property, value, strict);
                }
                return match property {
                    PropertyKey::String(atom) => {
                        let written = self.set_property_with_receiver(
                            p,
                            delegate_target,
                            atom,
                            value,
                            receiver,
                        )?;
                        if !written && strict {
                            return Err(self.rejected_assignment_error(
                                p,
                                self.heap.root_value(target).unwrap(),
                                atom,
                                true,
                            ));
                        }
                        Ok(written)
                    }
                    PropertyKey::Symbol(_) => self.set_symbol_property_with_receiver(
                        p,
                        delegate_target,
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
                if strict {
                    let property = self.detail_string(self.heap.root_value(key).unwrap());
                    return Err(self.type_error(
                        p,
                        format!("'set' on proxy: trap returned falsish for property '{property}'"),
                    ));
                }
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
        if self.is_host_private_symbol(key) {
            if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) {
                return Ok(Value::UNDEFINED);
            }
            let Some((value, attributes)) = self.ordinary_own_property(p, object, key)? else {
                return Ok(Value::UNDEFINED);
            };
            return if attributes.accessor {
                match attributes.getter.filter(|value| !value.is_undefined()) {
                    Some(getter) => self.call_value(p, getter, receiver, &[]),
                    None => Ok(Value::UNDEFINED),
                }
            } else {
                Ok(value)
            };
        }
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
            owner = data.proto();
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
            let property = self.atom_name(atom).to_owned();
            return Err(self.nullish_property_error(p, object, Some(&property)));
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
            self.check_private_brand_for_read(p, private_target, atom)?;
            if let Some(value) = self.private_field_value(p, private_target, atom) {
                return Ok(value);
            }
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
                && let Some(elements) = self.heap.array_elements(object)
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
            if let Some(data) = self.heap.object_record(object) {
                object = data.proto();
            } else if object.direct_array_index().is_some() {
                object = self
                    .heap
                    .object(object)
                    .expect("live direct array retains its object header")
                    .proto();
            } else {
                match self.heap.get(object) {
                    Some(Cell::ArrayBuffer { object: x, .. }) => object = x.proto(),
                    Some(Cell::TypedArray { object: x, .. }) => object = x.proto(),
                    Some(Cell::DataView { object: x, .. }) => object = x.proto(),
                    Some(Cell::String(v)) => {
                        let length = v.len();
                        if let Some(index) = super::object_static::array_index(self.atom_name(atom))
                            && let Some(unit) = self.string_index_value(object, index as usize)
                        {
                            return Ok(unit);
                        }
                        if atom == self.length_atom {
                            return Ok(Value::number(length as f64));
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
                                    .map(|value| {
                                        self.heap.alloc(Cell::String(value.as_ref().into()))
                                    })
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
                    | Some(Cell::TemporalInstant { object: x, .. }) => object = x.proto(),
                    Some(Cell::Array { object: x, .. }) => object = x.proto(),
                    Some(Cell::ShadowRealm { object: x, .. }) => object = x.proto(),
                    Some(Cell::RegExp { object: x, .. }) => object = x.proto(),
                    Some(Cell::Map { object: x, .. }) | Some(Cell::Set { object: x, .. }) => {
                        object = x.proto()
                    }
                    Some(Cell::WeakMap { object: x, .. })
                    | Some(Cell::WeakSet { object: x, .. }) => object = x.proto(),
                    Some(Cell::WeakRef { object: x, .. }) => object = x.proto(),
                    Some(Cell::FinalizationRegistry { object: x, .. }) => object = x.proto(),
                    Some(Cell::Iterator { object: x, .. }) => object = x.proto(),
                    Some(Cell::Function { object: x, .. }) => object = x.proto(),
                    _ => match self.heap.object(object) {
                        Some(x) => object = x.proto(),
                        None => return Ok(Value::UNDEFINED),
                    },
                }
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

    pub(super) fn private_field_value(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Option<Value> {
        let home = self.private_brand_home(p, target, atom)?;
        self.object_data(target)?
            .private_field(PrivateBrand { home, name: atom })
    }

    pub(super) fn set_private_field_value(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
        value: Value,
    ) -> bool {
        let Some(home) = self.private_brand_home(p, target, atom) else {
            return false;
        };
        self.object_data_mut(target).is_some_and(|object| {
            object.set_private_field(PrivateBrand { home, name: atom }, value)
        })
    }

    pub(super) fn initialize_private_field(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        home: Value,
        atom: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        let Some(object) = self.object_data_mut(target) else {
            return Err(self.type_error(p, "Cannot add private field to a non-object".into()));
        };
        if object.initialize_private_field(PrivateBrand { home, name: atom }, value) {
            return Ok(());
        }
        let name = crate::bytecode::private_identity_label(self.atom_name(atom))
            .unwrap_or_else(|| self.atom_name(atom).to_owned());
        Err(self.type_error(
            p,
            format!("Cannot initialize #{name} twice on the same object"),
        ))
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

    /// Report a failed private read with V8/Node's public error wording. Keep
    /// `check_private_brand` unchanged for writes and brand checks whose error
    /// semantics are distinct from reading a private member.
    pub(super) fn check_private_brand_for_read(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Result<(), JsError> {
        if self.has_private_brand(p, target, atom) {
            return Ok(());
        }
        let name = self.atom_name(atom);
        let member = if name.starts_with('#') {
            name.to_owned()
        } else {
            format!("#{name}")
        };
        Err(self.type_error(
            p,
            format!(
                "Cannot read private member {member} from an object whose class did not declare it"
            ),
        ))
    }

    pub(super) fn has_private_brand(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> bool {
        self.private_brand_home(p, target, atom).is_some()
    }

    /// The home object of `target`'s private name `atom` as seen from the running frame,
    /// if `target` carries it.
    pub(super) fn private_brand_home(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Option<Value> {
        let home = self.private_name_home(p, atom)?;
        self.object_data(target)
            .is_some_and(|object| object.has_private_name(PrivateBrand { home, name: atom }))
            .then_some(home)
    }

    /// The home object that declares private name `atom` for the running frame: the nearest
    /// eval-introduced private binding, otherwise the first visible class home declaring the
    /// name. It depends on the frame's function and environment chain, never on the target,
    /// so a found home is memoized while that environment lives.
    fn private_name_home(&mut self, p: &ResidualProgram, atom: Atom) -> Option<Value> {
        let frame_index = self.frames.len().checked_sub(1)?;
        if let Some(binding) = self.name_binding(frame_index, atom)
            && binding.kind == crate::bytecode::LexicalBindingKind::PrivateName
        {
            return self.load_name_binding(p, frame_index, binding).ok();
        }
        let start = self.captured_parent_environment(frame_index);
        let key = PrivateHomeKey {
            environment: start,
            program: self.frames[frame_index].program,
            function: self.frames[frame_index].function,
            name: atom,
        };
        if let Some(home) = self.private_homes.get(&key) {
            return Some(*home);
        }
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
        let mut environment = start;
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
                return Some(*home);
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
        let home = homes.into_iter().map(|(_, home)| home).find(|home| {
            self.object_data(*home).is_some_and(|object| {
                object.has_private_name(PrivateBrand {
                    home: *home,
                    name: atom,
                })
            })
        })?;
        // Eval-introduced private bindings live only in environments made for that eval, so
        // a cached chain never gains a nearer one.
        self.private_homes.insert(key, home);
        Some(home)
    }

    pub(super) fn get_private_proxy_field(
        &mut self,
        p: &ResidualProgram,
        proxy: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        if let Some(value) = self.private_field_value(p, proxy, atom) {
            return Ok(value);
        }
        let Some(home) = self.private_brand_home(p, proxy, atom) else {
            let name = self.atom_name(atom);
            let member = if name.starts_with('#') {
                name.to_owned()
            } else {
                format!("#{name}")
            };
            return Err(self.type_error(
                p,
                format!(
                    "Cannot read private member {member} from an object whose class did not declare it"
                ),
            ));
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
        // Methods live on the home object, which does not carry its own brand.
        self.own_property(home, atom).ok_or_else(|| {
            self.type_error(p, "private member is not present on this object".into())
        })
    }
}
