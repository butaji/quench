use super::operations::ArrayLikeElementKind;
use super::property_key::PropertyKey;
use super::*;

const INTERNAL_PROPERTY_PREFIX: &str = "\0quench:";

pub(super) enum PropertyCopyKind {
    Set,
    CreateDataProperty,
}

impl<H: Host> Vm<H> {
    pub(super) fn is_host_private_symbol(&self, key: Value) -> bool {
        matches!(self.heap.get(key), Some(Cell::Symbol(data))
            if data.kind == crate::heap::SymbolKind::HostPrivate)
    }

    /// An array of `keys`. Allocation never collects, so the keys need no roots meanwhile.
    pub(super) fn own_keys_array(&mut self, keys: Vec<Value>) -> Value {
        self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(Self::empty_object(self.array_proto), keys),
        })
    }

    fn is_internal_property_name(&self, atom: Atom) -> bool {
        self.atom_class(atom).contains(AtomClass::INTERNAL)
            && self.atom_name(atom).starts_with(INTERNAL_PROPERTY_PREFIX)
    }

    pub(super) fn own_named_keys(&self, object: Value) -> Vec<Atom> {
        let Some(data) = self.object_data(object) else {
            return Vec::new();
        };
        self.ordered_shape(data)
            .into_iter()
            .filter(|(atom, slot)| {
                self.heap.property_get(data, *slot).is_some()
                    && !self.is_internal_property_name(*atom)
            })
            .map(|(atom, _)| atom)
            .collect()
    }

    /// The engine-internal names an object owns, hidden from every key enumeration.
    pub(super) fn internal_named_keys(&self, object: Value) -> Vec<Atom> {
        let Some(data) = self.object_data(object) else {
            return Vec::new();
        };
        self.ordered_shape(data)
            .into_iter()
            .filter(|(atom, slot)| {
                self.heap.property_get(data, *slot).is_some()
                    && self.is_internal_property_name(*atom)
            })
            .map(|(atom, _)| atom)
            .collect()
    }

    /// The own symbol keys of an ordinary `object` of `shape`, in order: the shape's symbol keys
    /// the object still holds, but the host's private ones.
    fn ordinary_own_symbol_keys(&self, object: Value, shape: u32) -> Vec<Value> {
        self.shape_keys(shape)
            .iter()
            .filter_map(|key| key.symbol_value())
            .filter(|symbol| !self.is_host_private_symbol(*symbol) && self.symbol_property(object, *symbol).is_some())
            .collect()
    }

    pub(super) fn ordinary_own_key_values(
        &mut self,
        object: Value,
        include_indices: bool,
    ) -> Vec<Value> {
        let mut values = if include_indices {
            self.indexed_name_keys(object).unwrap_or_default()
        } else if self.heap.array(object).is_some() && self.own_array_length(object).is_some() {
            // Array length is a virtual own string key, not an array index.
            // Keep it before named properties without expanding element keys.
            vec![self.heap.alloc_string("length".into())]
        } else {
            Vec::new()
        };
        let Some(data) = self.object_data(object) else {
            return values;
        };
        let shape = data.shape();
        let strings = self
            .own_named_keys(object)
            .into_iter()
            .filter(|atom| {
                include_indices
                    || super::object_static::array_index(self.atom_name(*atom)).is_none()
            })
            .collect::<Vec<_>>();
        let symbols = self.ordinary_own_symbol_keys(object, shape);
        values.extend(
            strings
                .into_iter()
                .map(|atom| self.heap.alloc_string(self.atom_value(atom))),
        );
        values.extend(symbols);
        values
    }

    pub(super) fn proxy_own_keys(
        &mut self,
        p: &ResidualProgram,
        proxy: Value,
    ) -> Result<Vec<Value>, JsError> {
        let _stack = self.enter_stack()?;
        let Some((target, handler)) = self.proxy_parts(proxy) else {
            unreachable!("Proxy own-key dispatch");
        };
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        let proxy = self.heap.root(proxy);
        let target = self.heap.root(target);
        let handler = self.heap.root(handler);
        let mut keys = Vec::new();
        let mut target_keys = Vec::new();
        let outcome = (|| {
            let trap_atom = self.intern_atom("ownKeys");
            let trap = self.get_property(p, self.heap.root_value(handler).unwrap(), trap_atom)?;
            if trap.is_undefined() || trap.is_null() {
                return self.object_own_key_values(p, self.heap.root_value(target).unwrap());
            }
            if !self.is_function(trap) {
                return Err(self.type_error(p, "proxy ownKeys trap is not callable".into()));
            }
            let result = self.call_value(
                p,
                trap,
                self.heap.root_value(handler).unwrap(),
                &[self.heap.root_value(target).unwrap()],
            )?;
            keys = self
                .create_list_from_array_like(p, result, ArrayLikeElementKind::PropertyKey)?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            for (index, key) in keys.iter().enumerate() {
                if keys[..index].iter().any(|previous| {
                    self.same_property_key(
                        self.heap.root_value(*previous).unwrap(),
                        self.heap.root_value(*key).unwrap(),
                    )
                }) {
                    return Err(
                        self.type_error(p, "proxy ownKeys result contains duplicate keys".into())
                    );
                }
            }
            let extensible =
                self.object_is_extensible(p, &[self.heap.root_value(target).unwrap()])?;
            let extensible = self.truthy(extensible);
            target_keys = self
                .object_own_key_values(p, self.heap.root_value(target).unwrap())?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            let mut required = Vec::new();
            for key in &target_keys {
                let descriptor = self.object_get_own_property_descriptor(
                    p,
                    &[
                        self.heap.root_value(target).unwrap(),
                        self.heap.root_value(*key).unwrap(),
                    ],
                )?;
                if !extensible
                    || (!descriptor.is_undefined()
                        && !self.descriptor_flag(descriptor, "configurable"))
                {
                    required.push(*key);
                }
            }
            for key in required {
                if !keys.iter().any(|listed| {
                    self.same_property_key(
                        self.heap.root_value(*listed).unwrap(),
                        self.heap.root_value(key).unwrap(),
                    )
                }) {
                    return Err(
                        self.type_error(p, "proxy ownKeys trap omitted a required key".into())
                    );
                }
            }
            if !extensible && keys.len() != target_keys.len() {
                return Err(self.type_error(
                    p,
                    "proxy ownKeys trap changed the keys of a sealed target".into(),
                ));
            }
            Ok(keys
                .iter()
                .map(|key| self.heap.root_value(*key).unwrap())
                .filter(|key| !self.is_host_private_symbol(*key))
                .collect())
        })();
        for root in keys
            .into_iter()
            .chain(target_keys)
            .chain([proxy, target, handler])
        {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn object_own_key_values(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Vec<Value>, JsError> {
        let keys = self.object_own_keys(p, object)?;
        Ok(self.heap.array_elements(keys).map_or_else(Vec::new, |elements| {
            elements
                .iter()
                .copied()
                .filter(|key| {
                    !self.heap.string(*key).is_some_and(|name| name.host_string().starts_with("\0quench:"))
                })
                .collect()
        }))
    }

    pub(super) fn same_property_key(&self, left: Value, right: Value) -> bool {
        if let (Some(l), Some(r)) = (self.heap.string(left), self.heap.string(right)) {
            return l == r;
        }
        match (self.heap.get(left), self.heap.get(right)) {
            (Some(Cell::Symbol(_)), Some(Cell::Symbol(_))) => left == right,
            _ => false,
        }
    }

    pub(super) fn copy_data_properties(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        source: Value,
        exclusions: Value,
    ) -> Result<(), JsError> {
        let excluded = self
            .heap
            .array_elements(exclusions)
            .map_or_else(Vec::new, |elements| elements.to_vec());
        self.copy_enumerable_properties(
            p,
            target,
            source,
            &excluded,
            PropertyCopyKind::CreateDataProperty,
        )
    }

    pub(super) fn copy_enumerable_properties(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        source: Value,
        excluded: &[Value],
        kind: PropertyCopyKind,
    ) -> Result<(), JsError> {
        if source.is_null() || source.is_undefined() {
            return Ok(());
        }
        let source = self.box_object(source)?;
        let target = self.heap.root(target);
        let source = self.heap.root(source);
        let excluded = excluded
            .iter()
            .map(|key| self.heap.root(*key))
            .collect::<Vec<_>>();
        let mut keys = Vec::new();
        let outcome = (|| {
            keys = self
                .object_own_key_values(p, self.heap.root_value(source).unwrap())?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            for key in &keys {
                if excluded.iter().any(|excluded| {
                    self.same_property_key(
                        self.heap.root_value(*excluded).unwrap(),
                        self.heap.root_value(*key).unwrap(),
                    )
                }) {
                    continue;
                }
                let enumerable = self.own_property_enumerable(
                    p,
                    self.heap.root_value(source).unwrap(),
                    self.heap.root_value(*key).unwrap(),
                )?;
                if enumerable != Some(true) {
                    continue;
                }
                let value = self.get_index(
                    p,
                    self.heap.root_value(source).unwrap(),
                    self.heap.root_value(*key).unwrap(),
                )?;
                let target = self.heap.root_value(target).unwrap();
                let key = self.heap.root_value(*key).unwrap();
                match kind {
                    PropertyCopyKind::Set => self.set_index_mode(p, target, key, value, true)?,
                    PropertyCopyKind::CreateDataProperty => self.define_property_or_throw(
                        p,
                        target,
                        key,
                        super::object_descriptors::PropertyDescriptorRecord::data(value),
                    )?,
                }
            }
            Ok(())
        })();
        for root in keys.into_iter().chain(excluded).chain([target, source]) {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn symbol_property(&self, object: Value, key: Value) -> Option<Value> {
        let data = self.object_data(object)?;
        let slot = self.property_shape_slot(data.shape(), PropertyKey::symbol(key))?;
        self.slot_data_value(data, slot)
    }

    pub(super) fn set_symbol_property(
        &mut self,
        object: Value,
        key: Value,
        value: Value,
    ) -> Result<(), JsError> {
        self.set_shape_property(object, PropertyKey::symbol(key), value)
    }

    /// `[[Set]]` of a symbol key on its own receiver along a chain of non-Proxy objects, where
    /// every step is a shape fact: an own data property is written, and an absent key is added
    /// once the chain is proved to hold it as neither an accessor nor read-only
    /// (`symbol_write_proofs`). `None` leaves an accessor, a Proxy, a non-extensible receiver
    /// or a primitive to the general path, which runs no guest code before reaching one.
    pub(super) fn set_plain_symbol_property(&mut self, object: Value, key: Value, value: Value) -> Option<bool> {
        let symbol = PropertyKey::symbol(key);
        if self.proxy_parts(object).is_some() {
            return None;
        }
        let data = self.object_data(object)?;
        if !data.is_extensible() {
            return None;
        }
        let (shape, proto) = (data.shape(), data.proto());
        if let Some(slot) = self.property_shape_slot(shape, symbol) {
            let attributes = self.slot_attributes(data, slot)?;
            if attributes.accessor {
                return None;
            }
            if attributes.writable {
                self.heap.property_set(object, slot, value);
            }
            return Some(attributes.writable);
        }
        let epoch = crate::heap::prototype_layout_epoch();
        if !self.symbol_write_proofs.holds(proto, key, epoch) {
            let guard = self.add_prototype_guard(object, symbol)?;
            self.mark_prototype_links(proto, guard.prototype_depth);
            // Marking a link that was no prototype yet leaves the epoch where it was.
            if crate::heap::prototype_layout_epoch() == epoch {
                self.symbol_write_proofs.record(proto, key, epoch);
            }
        }
        self.create_shape_data_property(object, symbol, value, DEFAULT_DATA_ATTRIBUTES).ok()?;
        Some(true)
    }

    /// `object[key] = value` for a symbol `key` that `set_plain_symbol_property` completes as
    /// a successful write; `false` leaves any other store, unobserved, to the general path.
    pub(super) fn try_plain_symbol_set(&mut self, object: Value, key: Value, value: Value) -> bool {
        matches!(self.heap.get(key), Some(Cell::Symbol(symbol)) if symbol.kind != crate::heap::SymbolKind::HostPrivate)
            && self.set_plain_symbol_property(object, key, value) == Some(true)
    }

    /// `try_plain_symbol_set` at lane site `site` by the step the site recorded for the
    /// receiver's shape (`record_site_symbol_set`): the same store, or the same add, that
    /// `set_plain_symbol_property` makes for an extensible receiver of that shape and, for an
    /// add, prototype. `false` for any other write.
    #[inline(always)]
    pub(super) fn site_symbol_set(&mut self, site: usize, object: Value, key: Value, value: Value) -> bool {
        use super::symbol_write_proof::SymbolWriteStep;
        let Some(data) = self.heap.ordinary_object(object) else {
            return false;
        };
        if !data.is_extensible() {
            return false;
        }
        let epoch = crate::heap::prototype_layout_epoch();
        match self.symbol_write_proofs.site_step(site, data.shape(), key, epoch) {
            Some(SymbolWriteStep::Replace(slot)) => self.heap.property_set(object, slot as usize, value),
            Some(SymbolWriteStep::Add { prototype, target }) if data.proto() == prototype => {
                self.heap.property_add(object, value, target);
            }
            _ => return false,
        }
        true
    }

    /// `try_plain_symbol_set` at lane site `site`, recording the step it made for
    /// `site_symbol_set`: a store to a writable own data slot, or the add a write proof
    /// admitted, on an extensible receiver whose shapes are not dictionaries.
    pub(super) fn try_site_symbol_set(&mut self, site: usize, object: Value, key: Value, value: Value) -> bool {
        use super::symbol_write_proof::SymbolWriteStep;
        let before = self.heap.ordinary_object(object).map(Object::shape);
        if !self.try_plain_symbol_set(object, key, value) {
            return false;
        }
        let (Some(before), Some(data)) = (before, self.heap.ordinary_object(object)) else {
            return true;
        };
        let (after, prototype) = (data.shape(), data.proto());
        if !data.is_extensible() || self.shape_is_dictionary(before) || self.shape_is_dictionary(after) {
            return true;
        }
        let epoch = crate::heap::prototype_layout_epoch();
        let step = if after == before {
            let Some(slot) = self.property_shape_slot(before, PropertyKey::symbol(key)) else {
                return true;
            };
            let writable = self
                .slot_attributes(data, slot)
                .is_some_and(|attributes| !attributes.accessor && attributes.writable);
            if !writable {
                return true;
            }
            SymbolWriteStep::Replace(slot as u32)
        } else if self.symbol_write_proofs.holds(prototype, key, epoch) {
            SymbolWriteStep::Add { prototype, target: after }
        } else {
            return true;
        };
        self.symbol_write_proofs.record_site_step(site, before, key, epoch, step);
        true
    }

    pub(super) fn set_symbol_property_with_receiver(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
        value: Value,
        receiver: Value,
    ) -> Result<bool, JsError> {
        if self.is_host_private_symbol(key) {
            if matches!(self.heap.get(target), Some(Cell::Proxy { .. })) { return Ok(false); }
            let own = self.ordinary_own_property(p, target, key)?;
            if let Some((_, attributes)) = own {
                if attributes.accessor {
                    if let Some(setter) = attributes.setter.filter(|value| !value.is_undefined()) {
                        self.call_value(p, setter, receiver, &[value])?;
                        return Ok(true);
                    }
                    return Ok(false);
                }
                if !attributes.writable { return Ok(false); }
            }
            if !self.is_object_like(receiver) || matches!(self.heap.get(receiver), Some(Cell::Proxy { .. })) { return Ok(false); }
            let descriptor = if self.symbol_property(receiver, key).is_some() {
                super::object_descriptors::PropertyDescriptorRecord::value(value)
            } else {
                let mut descriptor = super::object_descriptors::PropertyDescriptorRecord::data(value);
                descriptor.enumerable = Some(false);
                descriptor
            };
            return self.define_own_property_record(p, receiver, key, descriptor);
        }
        if target == receiver
            && let Some(succeeded) = self.set_plain_symbol_property(target, key, value)
        {
            return Ok(succeeded);
        }
        let target_root = self.heap.root(target);
        let key_root = self.heap.root(key);
        let value_root = self.heap.root(value);
        let receiver_root = self.heap.root(receiver);
        let outcome = (|| {
            let mut current = target;
            loop {
                if let Some((target, handler)) = self.proxy_parts(current) {
                    return self.proxy_set(
                        p,
                        target,
                        handler,
                        self.heap.root_value(receiver_root).unwrap(),
                        PropertyKey::symbol(self.heap.root_value(key_root).unwrap()),
                        self.heap.root_value(value_root).unwrap(),
                    );
                }
                // A symbol key on a non-Proxy object: the own property's value and
                // attributes, without the descriptor object `[[GetOwnProperty]]` would build.
                if let Some((_, attributes)) = self.ordinary_own_property(p, current, key)? {
                    if attributes.accessor {
                        let Some(setter) =
                            attributes.setter.filter(|setter| !setter.is_undefined())
                        else {
                            return Ok(false);
                        };
                        self.call_value(
                            p,
                            setter,
                            self.heap.root_value(receiver_root).unwrap(),
                            &[self.heap.root_value(value_root).unwrap()],
                        )?;
                        return Ok(true);
                    }
                    if !attributes.writable {
                        return Ok(false);
                    }
                    break;
                }
                current = self.object_get_prototype_of(p, current)?;
                if current.is_null() {
                    break;
                }
            }

            if !self.is_object_like(receiver) {
                return Ok(false);
            }
            if matches!(self.heap.get(receiver), Some(Cell::Proxy { .. })) {
                return self.set_receiver_proxy_data_property(
                    p,
                    self.heap.root_value(receiver_root).unwrap(),
                    PropertyKey::symbol(self.heap.root_value(key_root).unwrap()),
                    self.heap.root_value(value_root).unwrap(),
                );
            }
            if let Some(attributes) = self.property_attributes(receiver, PropertyKey::symbol(key)) {
                if attributes.accessor || !attributes.writable {
                    return Ok(false);
                }
            } else if self.symbol_property(receiver, key).is_none()
                && !self
                    .object_data(receiver)
                    .is_some_and(Object::is_extensible)
            {
                return Ok(false);
            }
            self.set_symbol_property(receiver, key, value)?;
            Ok(true)
        })();
        for root in [target_root, key_root, value_root, receiver_root] {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn object_symbols(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) {
            let values = self
                .object_own_key_values(p, object)?
                .into_iter()
                .filter(|key| matches!(self.heap.get(*key), Some(Cell::Symbol(_))))
                .collect();
            return Ok(self.own_keys_array(values));
        }
        // An ordinary object's symbol keys are its shape's: no string key is made for them.
        let target = self.box_object(object)?;
        self.evaluate_deferred_namespace_for_key(p, target, None)?;
        self.complete_lazy_keys(p, target)?;
        let symbols = match self.object_data(target) {
            Some(data) => self.ordinary_own_symbol_keys(target, data.shape()),
            None => Vec::new(),
        };
        Ok(self.own_keys_array(symbols))
    }

    pub(super) fn object_own_keys(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) {
            let keys = self.proxy_own_keys(p, object)?;
            return Ok(self.own_keys_array(keys));
        }
        let target = self.box_object(object)?;
        self.evaluate_deferred_namespace_for_key(p, target, None)?;
        self.complete_lazy_keys(p, target)?;
        let values = self.ordinary_own_key_values(target, true);
        Ok(self.own_keys_array(values))
    }
}
