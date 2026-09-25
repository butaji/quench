use super::property_key::PropertyKey;
use super::*;
use crate::heap::PrivateBrand;

impl<H: Host> Vm<H> {
    pub(super) fn module_binding_value(&self, object: Value, atom: Atom) -> Option<Value> {
        let (program, slot) = self.object_data(object)?.module_binding(atom)?;
        let environment = self.programs.module_environment(program)?;
        let Some(Cell::Environment { slots, .. }) = self.heap.get(environment) else {
            return None;
        };
        slots.get(slot as usize).copied()
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

    pub(super) fn proxy_get(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        if handler.is_null() {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        let trap = self.proxy_trap(p, handler, "get")?;
        if self.is_function(trap) {
            let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
            return self.call_value(p, trap, handler, &[target, key, receiver]);
        }
        self.get_property_with_receiver(p, target, atom, receiver)
    }

    pub(super) fn proxy_set(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        atom: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        if self.atom_name(atom).starts_with("\0rqj:private:") {
            return Err(self.type_error(p, "private member is not present on this object".into()));
        }
        if handler.is_null() {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        let trap = self.proxy_trap(p, handler, "set")?;

        if let Some(Cell::Proxy {
            handler: current, ..
        }) = self.heap.get(receiver)
            && *current != handler
        {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        if self.is_function(trap) {
            let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
            let result = self.call_value(p, trap, handler, &[target, key, value, receiver])?;
            if !self.truthy(result) {
                return Err(JsError("proxy set trap returned false".into()));
            }
            return Ok(());
        }
        self.set_property_with_program(p, target, atom, value)
    }

    pub(super) fn proxy_get_symbol(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        key: Value,
    ) -> Result<Value, JsError> {
        if handler.is_null() {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        let trap = self.proxy_trap(p, handler, "get")?;
        if self.is_function(trap) {
            return self.call_value(p, trap, handler, &[target, key, receiver]);
        }
        Ok(self
            .symbol_property(target, key)
            .unwrap_or(Value::UNDEFINED))
    }

    pub(super) fn proxy_set_symbol(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        key: Value,
        value: Value,
    ) -> Result<(), JsError> {
        if handler.is_null() {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        let trap = self.proxy_trap(p, handler, "set")?;
        if self.is_function(trap) {
            let result = self.call_value(p, trap, handler, &[target, key, value, receiver])?;
            if !self.truthy(result) {
                return Err(JsError("proxy set trap returned false".into()));
            }
            return Ok(());
        }
        self.set_symbol_property(target, key, value)
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
        let private_name = self.atom_name(atom).starts_with("\0rqj:private:");
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
            }) = self.heap.get(object).cloned()
            {
                // Private names are not property keys observable through a
                // Proxy. Forwarding here would incorrectly let the target's
                // hidden storage satisfy a private access on the Proxy.
                if private_name {
                    return Err(
                        self.type_error(p, "private member is not present on this object".into())
                    );
                }
                return self.proxy_get(p, target, handler, receiver, atom);
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
            if let Some(value) = self.module_binding_value(object, atom) {
                if value.is_deleted() {
                    return Err(self.reference_error(
                        p,
                        format!(
                            "Cannot access '{}' before initialization",
                            self.atom_name(atom)
                        ),
                    ));
                }
                return Ok(value);
            }
            if let Some(index) = super::object_static::array_index(self.atom_name(atom))
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
            if let Some(v) = self.own_property(object, atom) {
                return Ok(v);
            }
            if let Some(v) = self.indexed_view_property(object, atom) {
                return Ok(v);
            }
            match self.heap.get(object) {
                Some(Cell::ArrayBuffer { .. })
                    if self.array_buffer_virtual_property(object, atom).is_some() =>
                {
                    return Ok(self.array_buffer_virtual_property(object, atom).unwrap());
                }
                Some(Cell::ArrayBuffer { bytes, shared, .. }) if atom == self.byte_length_atom => {
                    let _shared = shared;
                    return Ok(Value::number(if self.array_buffer_detached(object) {
                        0.0
                    } else {
                        bytes.len() as f64
                    }));
                }
                Some(Cell::Array { .. })
                    if atom == self.length_atom
                        && !self
                            .object_data(object)
                            .is_some_and(Object::is_arguments_object) =>
                {
                    let Some(Cell::Array { elements, .. }) = self.heap.get(object) else {
                        unreachable!()
                    };
                    let length = self.heap.sparse_length(object).unwrap_or(elements.len());
                    return Ok(Value::number(length as f64));
                }
                Some(Cell::Map { entries, .. }) if atom == self.size_atom => {
                    return Ok(Value::number(entries.len() as f64));
                }
                Some(Cell::ArrayBuffer { object: x, .. }) => object = x.proto,
                Some(Cell::TypedArray { object: x, .. }) => object = x.proto,
                Some(Cell::DataView { object: x, .. }) => object = x.proto,
                Some(Cell::Set { entries, .. }) if atom == self.size_atom => {
                    return Ok(Value::number(entries.len() as f64));
                }
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
                Some(Cell::Date { object: x, .. }) => {
                    let native = self.date_property_native(atom);
                    if !native.is_undefined() {
                        return Ok(native);
                    }
                    object = x.proto;
                }
                Some(Cell::Object(x))
                | Some(Cell::Array { object: x, .. })
                | Some(Cell::RegExp { object: x, .. }) => object = x.proto,
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
            dynamic_bindings,
            ..
        }) = self.heap.get(environment)
        {
            if let Some((_, home)) = dynamic_bindings
                .iter()
                .rev()
                .find(|(binding, _)| *binding == private_home_binding)
            {
                return self
                    .object_data(target)
                    .is_some_and(|object| {
                        object.private_names.contains(&PrivateBrand {
                            home: *home,
                            name: atom,
                        })
                    })
                    .then_some(*home);
            }
            if *function != u32::MAX {
                let visible_homes = p
                    .functions
                    .get(*function as usize)
                    .into_iter()
                    .flat_map(|metadata| metadata.local_atoms.iter().copied())
                    .filter(|atom| self.atom_name(*atom).starts_with("\0rqj:home:"))
                    .collect::<Vec<_>>();
                for atom in home_atoms.iter().chain(&visible_homes) {
                    if homes.iter().any(|(candidate, _)| candidate == atom) {
                        continue;
                    }
                    if let Some(slot) = self
                        .local_binding_slot(p, *function, *atom)
                        .filter(|slot| *slot < slots.len())
                    {
                        homes.push((*atom, slots[slot]));
                    }
                }
            }
            environment = *parent;
        }
        for (_, home) in homes.iter().copied() {
            let declares_name = self.object_data(home).is_some_and(|object| {
                object
                    .private_names
                    .contains(&PrivateBrand { home, name: atom })
            });
            if !declares_name {
                continue;
            }
            let branded = self.object_data(target).is_some_and(|object| {
                object
                    .private_names
                    .contains(&PrivateBrand { home, name: atom })
            });
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
