use super::property_key::PropertyKey;
use super::*;
impl<H: Host> Vm<H> {
    #[inline(always)]
    pub(super) fn shape_slot(&self, shape: u32, atom: Atom) -> Option<usize> {
        self.property_shape_slot(shape, PropertyKey::string(atom))
    }
    #[inline(always)]
    pub(super) fn property_shape_slot(&self, shape: u32, key: PropertyKey) -> Option<usize> {
        self.shapes
            .get(shape as usize)
            .and_then(|shape| shape.slots.get(&key).copied())
            .map(|slot| slot as usize)
    }
    #[inline(always)]
    pub(super) fn property_attributes(
        &self,
        object: Value,
        key: PropertyKey,
    ) -> Option<PropertyAttributes> {
        if let Some(data) = self.object_data(object)
            && let Some(slot) = self.property_shape_slot(data.shape(), key)
            && let Some(attributes) = self
                .shapes
                .get(data.shape() as usize)
                .and_then(|shape| shape.descriptors.get(slot))
        {
            return Some(*attributes);
        }
        self.descriptors.get(&(object, key)).copied()
    }
    pub(super) fn set_property_attributes(
        &mut self,
        object: Value,
        key: PropertyKey,
        attributes: PropertyAttributes,
    ) {
        if let Some(data) = self.object_data(object)
            && let Some(slot) = self.property_shape_slot(data.shape(), key)
        {
            let shape = data.shape();
            if self.shapes[shape as usize].descriptors[slot] == attributes {
                return;
            }
            let mut next = self.shapes[shape as usize].clone();
            next.descriptors[slot] = attributes;
            let next_id = self.shapes.len() as u32;
            self.shapes.push(next);
            self.heap
                .register_property_shape(next_id, self.shapes[next_id as usize].storage_len);
            self.object_data_mut(object)
                .expect("object survived descriptor transition")
                .set_shape(next_id);
            self.invalidate_field_caches();
            self.invalidate_method_caches_for_key(key);
            return;
        }
        self.descriptors.insert((object, key), attributes);
        self.invalidate_field_caches();
        self.invalidate_method_caches_for_key(key);
    }
    pub(super) fn remove_property_attributes(&mut self, object: Value, key: PropertyKey) {
        if let Some(data) = self.object_data(object)
            && let Some(slot) = self.property_shape_slot(data.shape(), key)
        {
            let shape = data.shape();
            let mut next = self.shapes[shape as usize].clone();
            next.descriptors[slot] = DEFAULT_PROPERTY_ATTRIBUTES;
            let next_id = self.shapes.len() as u32;
            self.shapes.push(next);
            self.heap
                .register_property_shape(next_id, self.shapes[next_id as usize].storage_len);
            self.object_data_mut(object)
                .expect("object survived descriptor transition")
                .set_shape(next_id);
            self.invalidate_field_caches();
            self.invalidate_method_caches_for_key(key);
            return;
        }
        if self.descriptors.remove(&(object, key)).is_some() {
            self.invalidate_field_caches();
            self.invalidate_method_caches_for_key(key);
        }
    }
    pub(super) fn invalidate_field_caches(&mut self) {
        self.field_caches.fill(EMPTY_CACHE);
        self.megamorphic_field_indices.fill(NO_MEGAMORPHIC_FIELD);
        self.megamorphic_fields.clear();
    }
    fn invalidate_method_caches_for_key(&mut self, key: PropertyKey) {
        match key {
            PropertyKey::String(atom) | PropertyKey::Private(atom) => {
                self.invalidate_method_caches_for_atom(atom);
            }
            PropertyKey::Symbol(_) => {}
        }
    }
    pub(super) fn checked_this_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
    ) -> Result<Value, JsError> {
        let value = self.frames[frame].this;
        if value.is_deleted() {
            return Err(self.reference_error(
                p,
                "Must call super constructor before accessing 'this'".into(),
            ));
        }
        Ok(value)
    }

    #[inline(always)]
    pub(super) fn resolve_field_base(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        base: FieldBase,
    ) -> Result<Value, JsError> {
        match base.register_index() {
            Some(register) => Ok(self.read(frame, register)),
            None => self.checked_this_binding(p, frame),
        }
    }
}
impl FieldCacheSet {
    fn with_pair(first: FieldCache, second: FieldCache) -> Self {
        let mut entries = [EMPTY_CACHE; FIELD_MEGAMORPHIC_INLINE];
        entries[0] = first;
        entries[1] = second;
        Self {
            len: 2,
            entries,
            overflow: None,
        }
    }
    #[inline(always)]
    pub(super) fn get(&self, shape: u32) -> Option<FieldCache> {
        if let Some(entries) = &self.overflow {
            return entries.get(&shape).copied();
        }
        self.entries[..usize::from(self.len)]
            .iter()
            .find(|entry| entry.receiver == shape)
            .copied()
    }
    fn insert(&mut self, cache: FieldCache) {
        const LIMIT: usize = 256;
        if let Some(entries) = &mut self.overflow {
            if entries.len() < LIMIT || entries.contains_key(&cache.receiver) {
                entries.insert(cache.receiver, cache);
            }
            return;
        }
        if let Some(entry) = self.entries[..usize::from(self.len)]
            .iter_mut()
            .find(|entry| entry.receiver == cache.receiver)
        {
            *entry = cache;
        } else if usize::from(self.len) < FIELD_MEGAMORPHIC_INLINE {
            self.entries[usize::from(self.len)] = cache;
            self.len += 1;
        } else {
            let mut entries = FxHashMap::default();
            entries.extend(self.entries.iter().map(|entry| (entry.receiver, *entry)));
            entries.insert(cache.receiver, cache);
            self.overflow = Some(Box::new(entries));
        }
    }
    #[cfg(any(feature = "profile-memory", test))]
    pub(super) fn len(&self) -> usize {
        self.overflow
            .as_ref()
            .map_or(usize::from(self.len), |entries| entries.len())
    }
}
impl<H: Host> Vm<H> {
    pub(super) fn object_pair(
        &mut self,
        program: &ResidualProgram,
        site: usize,
        first: Value,
        second: Value,
    ) -> Value {
        if !self.specialized {
            let [first_atom, second_atom] = program.object_sites[site].atoms;
            let one = self.transition_shape(0, first_atom);
            let two = self.transition_shape(one, second_atom);
            return self
                .heap
                .alloc_object_pair(self.object_proto, two, first, second);
        }
        let shape = if self.object_shapes[site] != u32::MAX {
            self.object_shapes[site]
        } else {
            let atoms = program.object_sites[site].atoms;
            let one = self.transition_shape(0, atoms[0]);
            let two = self.transition_shape(one, atoms[1]);
            self.object_shapes[site] = two;
            two
        };
        self.heap
            .alloc_object_pair(self.object_proto, shape, first, second)
    }
    pub(super) fn own_property(&self, object: Value, atom: Atom) -> Option<Value> {
        let object = self.object_data(object)?;
        let slot = self.shape_slot(object.shape(), atom)?;
        self.heap.property_get(object, slot)
    }
    pub(super) fn object_data(&self, value: Value) -> Option<&Object> {
        self.heap.get(value)?.object()
    }
    pub(super) fn object_data_mut(&mut self, value: Value) -> Option<&mut Object> {
        self.heap.get_mut(value)?.object_mut()
    }
    pub(super) fn is_object_like(&self, value: Value) -> bool {
        matches!(
            self.heap.get(value),
            Some(
                Cell::Object(_)
                    | Cell::Array { .. }
                    | Cell::ArrayBuffer { .. }
                    | Cell::TypedArray { .. }
                    | Cell::DataView { .. }
                    | Cell::Map { .. }
                    | Cell::Set { .. }
                    | Cell::WeakMap { .. }
                    | Cell::WeakSet { .. }
                    | Cell::WeakRef { .. }
                    | Cell::FinalizationRegistry { .. }
                    | Cell::Iterator { .. }
                    | Cell::Proxy { .. }
                    | Cell::Function { .. }
                    | Cell::Date { .. }
                    | Cell::RegExp { .. }
                    | Cell::Error(_)
            )
        )
    }
    pub(super) fn get_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Result<Value, JsError> {
        if self.atom_name(atom).starts_with("\0rqj:private:")
            && matches!(self.heap.get(object), Some(Cell::Proxy { .. }))
        {
            return self.get_private_proxy_field(p, object, atom);
        }
        if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) {
            return self.get_property(p, object, atom);
        }
        if !self.specialized {
            return self.get_property(p, object, atom);
        }
        if !object.is_heap()
            || atom == self.length_atom
            || atom == self.size_atom
            || atom == self.byte_length_atom
            || atom == self.byte_offset_atom
            || atom == self.buffer_atom
        {
            return self.get_property(p, object, atom);
        }
        let Some(receiver) = self.object_data(object) else {
            return self.get_property(p, object, atom);
        };
        let receiver_shape = receiver.shape();
        // SAFETY: cache-site ids are emitted only by the compiler and execute
        // against the exactly-sized cache vector initialized for this program.
        let cache = unsafe { *self.field_caches.get_unchecked(site as usize) };
        if cache.receiver == receiver_shape
            && let Some(owner) = self.field_cache_owner(object, cache)
            && let Some(owner_data) = self.object_data(owner)
            && self
                .heap
                .property_get(owner_data, cache.slot as usize)
                .is_some()
        {
            // SAFETY: receiver shape, owner identity, owner shape, and slot
            // were recorded together on the cache miss path.
            let value = unsafe {
                self.heap
                    .property_get_unchecked(owner_data, cache.slot as usize)
            };
            self.profile.field_cache_hit(0, 0);
            return Ok(value);
        }
        if let Some(cache) = self.megamorphic_field_cache(site, receiver_shape)
            && let Some(owner) = self.field_cache_owner(object, cache)
            && let Some(owner_data) = self.object_data(owner)
            && self
                .heap
                .property_get(owner_data, cache.slot as usize)
                .is_some()
        {
            // SAFETY: the table is keyed by the immutable receiver shape.
            let value = unsafe {
                self.heap
                    .property_get_unchecked(owner_data, cache.slot as usize)
            };
            self.profile.field_cache_hit(2, 0);
            return Ok(value);
        }
        self.profile.field_cache(false);
        self.get_field_miss(p, object, atom, site)
    }
    #[cold]
    #[inline(never)]
    pub(super) fn get_field_miss(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Result<Value, JsError> {
        if self.property_accessor(object, atom).is_some() {
            return self.get_property(p, object, atom);
        }
        if let Some(value) = self.array_buffer_virtual_property(object, atom) {
            return Ok(value);
        }
        if atom == self.byte_length_atom
            && let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(object)
        {
            return Ok(Value::number(if self.array_buffer_detached(object) {
                0.0
            } else {
                bytes.len() as f64
            }));
        }
        if let Some(value) = self.indexed_view_property(object, atom) {
            return Ok(value);
        }
        let mut owner = object;
        let mut depth = 0u16;
        loop {
            if matches!(self.heap.get(owner), Some(Cell::Proxy { .. })) {
                return self.get_property(p, object, atom);
            }
            let Some(current) = self.object_data(owner) else {
                return Ok(Value::UNDEFINED);
            };
            if let Some(slot) = self.shape_slot(current.shape(), atom)
                && let Some(value) = self.heap.property_get(current, slot)
            {
                if slot <= u16::MAX as usize {
                    let receiver = self
                        .object_data(object)
                        .map(Object::shape)
                        .unwrap_or(u32::MAX);
                    self.record_field_cache(
                        site,
                        FieldCache {
                            receiver,
                            atom,
                            owner,
                            owner_shape: current.shape(),
                            slot: slot as u16,
                            depth,
                        },
                    );
                }
                return Ok(value);
            }
            owner = current.proto;
            if owner.is_null() {
                return Ok(Value::UNDEFINED);
            }
            depth = depth.saturating_add(1);
        }
    }
    pub(super) fn set_property(
        &mut self,
        object: Value,
        atom: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        self.set_shape_property(object, PropertyKey::string(atom), value)
    }

    /// ECMAScript `OrdinarySet` with an explicit receiver. This is the shared
    /// write authority used by `Reflect.set` and `super` references: lookup is
    /// performed on `target`, while a writable data property is created or
    /// updated on `receiver`.
    pub(super) fn set_property_with_receiver(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
        value: Value,
        receiver: Value,
    ) -> Result<bool, JsError> {
        if self.atom_name(atom).starts_with("\0rqj:private:") {
            self.check_private_brand(p, target, atom)?;
        }
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(target).cloned()
        {
            return self
                .proxy_set(p, target, handler, receiver, atom, value)
                .map(|()| true);
        }
        self.evaluate_deferred_namespace_for_key(p, target, Some(PropertyKey::string(atom)))?;

        let mut current = target;
        let mut found = None;
        loop {
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = self.heap.get(current).cloned()
            {
                return self
                    .proxy_set(p, target, handler, receiver, atom, value)
                    .map(|()| true);
            }
            if let Some(attributes) = self.property_attributes(current, PropertyKey::string(atom))
                && (self.own_property(current, atom).is_some() || attributes.accessor)
            {
                found = Some((current, attributes));
                break;
            }
            let Some(data) = self.object_data(current) else {
                break;
            };
            current = data.proto;
            if current.is_null() {
                break;
            }
        }

        if let Some((owner, attributes)) = found {
            if attributes.accessor {
                let Some(setter) = attributes.setter else {
                    return Ok(false);
                };
                self.call_value(p, setter, receiver, &[value])?;
                return Ok(true);
            }
            if !attributes.writable {
                return Ok(false);
            }
            let _ = owner;
        }

        if self
            .object_data(receiver)
            .is_some_and(Object::is_module_namespace)
        {
            let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
            let descriptor = self.object_get_own_property_descriptor(p, &[receiver, key])?;
            if descriptor.is_undefined() {
                return Ok(false);
            }
            let current = self
                .descriptor_field(p, descriptor, "value")?
                .unwrap_or(Value::UNDEFINED);
            return Ok(self.same_value(current, value));
        }

        if self.object_data(receiver).is_none() {
            return Ok(false);
        }
        let receiver_descriptor = if matches!(self.heap.get(receiver), Some(Cell::Proxy { .. })) {
            let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
            Some(self.object_get_own_property_descriptor(p, &[receiver, key])?)
        } else {
            None
        };
        let receiver_has_own = receiver_descriptor
            .is_some_and(|descriptor| !descriptor.is_undefined())
            || self.own_property(receiver, atom).is_some();
        if receiver_has_own {
            if let Some(descriptor) = receiver_descriptor.filter(|value| !value.is_undefined()) {
                let getter = self.descriptor_field(p, descriptor, "get")?;
                let setter = self.descriptor_field(p, descriptor, "set")?;
                let writable = self
                    .descriptor_field(p, descriptor, "writable")?
                    .is_some_and(|value| self.truthy(value));
                if getter.is_some() || setter.is_some() || !writable {
                    return Ok(false);
                }
            } else if let Some(attributes) =
                self.property_attributes(receiver, PropertyKey::string(atom))
                && (attributes.accessor || !attributes.writable)
            {
                return Ok(false);
            }
            return self.define_receiver_data_property(p, receiver, atom, value, false);
        }
        if !self
            .object_data(receiver)
            .is_some_and(Object::is_extensible)
        {
            return Ok(false);
        }
        self.define_receiver_data_property(p, receiver, atom, value, true)
    }

    fn define_receiver_data_property(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        atom: Atom,
        value: Value,
        new_property: bool,
    ) -> Result<bool, JsError> {
        let descriptor = self.object();
        let value_atom = self.intern_atom("value");
        self.set_property(descriptor, value_atom, value)?;
        if new_property {
            for name in ["writable", "enumerable", "configurable"] {
                let field = self.intern_atom(name);
                self.set_property(descriptor, field, Value::TRUE)?;
            }
        }
        if atom == self.length_atom
            && matches!(self.heap.get(receiver), Some(Cell::Array { .. }))
            && !self
                .object_data(receiver)
                .is_some_and(Object::is_arguments_object)
        {
            return self.define_array_length(p, receiver, descriptor);
        }
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(receiver).cloned()
        {
            if handler.is_null() {
                return Err(self.type_error(p, "cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("defineProperty");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
                let result = self.call_value(p, trap, handler, &[target, key, descriptor])?;
                if !self.truthy(result) {
                    return Ok(false);
                }
                self.validate_proxy_define_property(p, target, key, descriptor)?;
                return Ok(true);
            }
        }
        let key = self.heap.alloc(Cell::String(self.atom_value(atom)));
        self.object_define_property(p, &[receiver, key, descriptor])?;
        self.mirror_global_var_property_write(p, receiver, atom, value);
        Ok(true)
    }

    pub(super) fn set_shape_property(
        &mut self,
        object: Value,
        key: PropertyKey,
        value: Value,
    ) -> Result<(), JsError> {
        let (slot, shape, exists) = {
            let data = self
                .object_data(object)
                .ok_or_else(|| JsError("property write on non-object".into()))?;
            let slot = self.property_shape_slot(data.shape(), key);
            let exists = slot.is_some_and(|slot| self.heap.property_get(data, slot).is_some());
            (slot, data.shape(), exists)
        };
        self.check_property_key_write(object, key, exists)?;
        if let Some(slot) = slot {
            self.heap.property_set(object, slot, value);
        } else {
            let next_shape = self.transition_property_shape(shape, key);
            self.heap.property_push(object, value);
            self.object_data_mut(object).unwrap().set_shape(next_shape);
        }
        self.invalidate_method_caches_for_key(key);
        Ok(())
    }

    pub(super) fn set_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
        strict: bool,
    ) -> Result<(), JsError> {
        // Assignment to a nullish base is an abrupt completion regardless of
        // strictness. Keep it on the canonical realm-owned TypeError path;
        // the generic object mutator's fallback error is not an ECMAScript
        // error object and loses constructor identity at the Test262 boundary.
        if object.is_null() || object.is_undefined() {
            return Err(self.type_error(
                p,
                if object.is_null() {
                    "cannot set properties of null".into()
                } else {
                    "cannot set properties of undefined".into()
                },
            ));
        }
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(object).cloned()
        {
            if self.atom_name(atom).starts_with("\0rqj:private:") {
                if self.own_property(object, atom).is_none() {
                    let extensible = self.object_is_extensible(p, &[object])?;
                    if !self.truthy(extensible) {
                        return Err(self.type_error(
                            p,
                            "Cannot add private field to a non-extensible object".into(),
                        ));
                    }
                }
                return self.set_shape_property(object, PropertyKey::string(atom), value);
            }
            return self.proxy_set(p, target, handler, object, atom, value);
        }
        if !self.is_object_like(object) {
            if self.atom_name(atom).starts_with("\0rqj:private:") {
                self.check_private_brand(p, object, atom)?;
            }
            let boxed = self.box_primitive_object(object)?;
            let succeeded = self.set_property_with_receiver(p, boxed, atom, value, object)?;
            return if succeeded || !strict {
                Ok(())
            } else {
                Err(self.type_error(p, "cannot assign property on primitive value".into()))
            };
        }
        if let Some(attributes) = self.property_accessor(object, atom) {
            if let Some(setter) = attributes.setter {
                self.call_value(p, setter, object, &[value])?;
            } else if strict || self.atom_name(atom).starts_with("\0rqj:private:") {
                return Err(self.type_error(p, "property has no setter".into()));
            }
            return Ok(());
        }
        let own = self.own_property(object, atom).is_some();
        if own
            && self
                .property_attributes(object, PropertyKey::string(atom))
                .is_some_and(|attributes| !attributes.writable)
        {
            return if strict || self.atom_name(atom).starts_with("\0rqj:private:") {
                Err(self.type_error(p, "cannot write non-writable property".into()))
            } else {
                Ok(())
            };
        }
        if !own && self.inherited_write_blocked(object, atom) {
            return if strict || self.atom_name(atom).starts_with("\0rqj:private:") {
                Err(self.type_error(p, "cannot write inherited non-writable property".into()))
            } else {
                Ok(())
            };
        }
        if !own
            && self
                .object_data(object)
                .is_some_and(|object| !object.is_extensible())
        {
            return if strict {
                Err(self.type_error(p, "cannot add property to non-extensible object".into()))
            } else {
                Ok(())
            };
        }
        if atom == self.length_atom
            && matches!(self.heap.get(object), Some(Cell::Array { .. }))
            && !self
                .object_data(object)
                .is_some_and(Object::is_arguments_object)
        {
            let succeeded = self.set_array_length(p, object, value)?;
            return if succeeded || !strict {
                Ok(())
            } else {
                Err(self.type_error(p, "cannot delete non-configurable array element".into()))
            };
        }
        if !self.specialized {
            return self.set_property_with_program(p, object, atom, value);
        }
        let existing = self
            .object_data(object)
            .and_then(|data| self.shape_slot(data.shape(), atom));
        self.check_property_write(object, atom, existing.is_some())?;
        let shape = self
            .object_data(object)
            .map(Object::shape)
            .unwrap_or(u32::MAX);
        let cache = self.field_caches[site as usize];
        if shape != u32::MAX && cache.receiver == shape && cache.depth == 0 {
            let invalidates_method = self.callable_write(object, cache.slot as usize, value);
            // SAFETY: a matching immutable shape proves the cached slot layout.
            unsafe {
                self.heap
                    .property_set_unchecked(object, cache.slot as usize, value);
            }
            if invalidates_method {
                self.invalidate_method_caches_for_atom(atom);
            }
            self.profile.field_cache_hit(0, 0);
            return Ok(());
        }
        if shape != u32::MAX
            && let Some(cache) = self.megamorphic_field_cache(site, shape)
        {
            let invalidates_method = self.callable_write(object, cache.slot as usize, value);
            // SAFETY: the megamorphic entry is keyed by this immutable shape.
            unsafe {
                self.heap
                    .property_set_unchecked(object, cache.slot as usize, value);
            }
            if invalidates_method {
                self.invalidate_method_caches_for_atom(atom);
            }
            self.profile.field_cache_hit(2, 0);
            return Ok(());
        }
        self.profile.field_cache(false);
        self.set_property(object, atom, value)?;
        let data = self.object_data(object).unwrap();
        let slot = self
            .shape_slot(data.shape(), atom)
            .expect("property transition records the new shape slot");
        let data_shape = data.shape();
        if slot <= u16::MAX as usize {
            self.record_field_cache(
                site,
                FieldCache {
                    receiver: data_shape,
                    atom,
                    owner: object,
                    owner_shape: data_shape,
                    slot: slot as u16,
                    depth: 0,
                },
            );
        }
        Ok(())
    }
    pub(super) fn set_property_with_program(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        self.set_property_with_program_mode(p, object, atom, value, false)
    }

    pub(super) fn set_property_with_program_mode(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        strict: bool,
    ) -> Result<(), JsError> {
        self.evaluate_deferred_namespace_for_key(p, object, Some(PropertyKey::string(atom)))?;
        if atom == self.length_atom
            && matches!(self.heap.get(object), Some(Cell::Array { .. }))
            && !self
                .object_data(object)
                .is_some_and(Object::is_arguments_object)
        {
            if !self.set_array_length(p, object, value)? && strict {
                return Err(self.type_error(p, "cannot set array length".into()));
            }
            return Ok(());
        }
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(object).cloned()
        {
            return self.proxy_set(p, target, handler, object, atom, value);
        }
        if let Some(attributes) = self.property_accessor(object, atom) {
            if let Some(setter) = attributes.setter {
                self.call_value(p, setter, object, &[value])?;
            }
            return Ok(());
        }
        if self.own_property(object, atom).is_none() && self.inherited_write_blocked(object, atom) {
            return Err(JsError(
                "cannot write inherited non-writable property".into(),
            ));
        }
        self.set_property(object, atom, value)?;
        self.mirror_global_var_property_write(p, object, atom, value);
        Ok(())
    }
    pub(super) fn property_accessor(
        &self,
        mut object: Value,
        atom: Atom,
    ) -> Option<PropertyAttributes> {
        loop {
            if let Some(attributes) = self.property_attributes(object, PropertyKey::string(atom)) {
                return attributes.accessor.then_some(attributes);
            }
            if self.own_property(object, atom).is_some() {
                return None;
            }
            object = self.object_data(object)?.proto;
            if object.is_null() {
                return None;
            }
        }
    }

    pub(super) fn inherited_write_blocked(&self, object: Value, atom: Atom) -> bool {
        let mut object = self.object_data(object).map(|data| data.proto);
        while let Some(current) = object.filter(|value| !value.is_null()) {
            if let Some(attributes) = self.property_attributes(current, PropertyKey::string(atom)) {
                return !attributes.accessor && !attributes.writable;
            }
            object = self.object_data(current).map(|data| data.proto);
        }
        false
    }
    fn callable_write(&self, object: Value, slot: usize, value: Value) -> bool {
        self.is_function(value)
            || self
                .object_data(object)
                .and_then(|object| self.heap.property_get(object, slot))
                .is_some_and(|old| self.is_function(old))
    }
    pub(super) fn record_field_cache(&mut self, site: u16, cache: FieldCache) {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors.
        let table_index = unsafe { *self.megamorphic_field_indices.get_unchecked(site as usize) };
        if table_index != NO_MEGAMORPHIC_FIELD {
            // SAFETY: every non-sentinel index is written when its table is pushed.
            let entries = unsafe {
                self.megamorphic_fields
                    .get_unchecked_mut(table_index as usize)
            };
            entries.insert(cache);
            return;
        }
        let entry = &mut self.field_caches[site as usize];
        if entry.receiver == cache.receiver || entry.receiver == u32::MAX {
            *entry = cache;
            return;
        }
        let table_index = self.megamorphic_fields.len() as u32;
        self.megamorphic_fields
            .push(FieldCacheSet::with_pair(*entry, cache));
        // SAFETY: compiler-produced sites index exactly-sized cache vectors.
        unsafe {
            *self
                .megamorphic_field_indices
                .get_unchecked_mut(site as usize) = table_index;
        }
    }
    #[inline(always)]
    fn megamorphic_field_cache(&self, site: u16, shape: u32) -> Option<FieldCache> {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors;
        // every non-sentinel index was installed together with its table.
        let table_index = unsafe { *self.megamorphic_field_indices.get_unchecked(site as usize) };
        if table_index == NO_MEGAMORPHIC_FIELD {
            return None;
        }
        unsafe {
            self.megamorphic_fields
                .get_unchecked(table_index as usize)
                .get(shape)
        }
    }
    pub(super) fn transition_shape(&mut self, shape: u32, atom: Atom) -> u32 {
        self.transition_property_shape(shape, PropertyKey::string(atom))
    }
    pub(super) fn transition_property_shape(&mut self, shape: u32, key: PropertyKey) -> u32 {
        if let Some(next) = self.transitions.get(&(shape, key)).copied() {
            self.profile.shape_transition(true);
            return next;
        }
        self.profile.shape_transition(false);
        let mut fields = self.shapes[shape as usize].keys.clone();
        let mut slots = self.shapes[shape as usize].slots.clone();
        let mut descriptors = self.shapes[shape as usize].descriptors.clone();
        let storage_len = self.shapes[shape as usize].storage_len;
        let slot = u32::try_from(storage_len).expect("object property index exceeds u32");
        slots.insert(key, slot);
        fields.push(key);
        descriptors.push(DEFAULT_PROPERTY_ATTRIBUTES);
        let next = self.shapes.len() as u32;
        self.shapes.push(Shape {
            keys: fields,
            slots,
            descriptors,
            storage_len: storage_len + 1,
        });
        self.heap.register_property_shape(next, storage_len + 1);
        self.transitions.insert((shape, key), next);
        next
    }
    pub(super) fn delete_shape_property(&mut self, object: Value, key: PropertyKey) {
        let Some(shape) = self.object_data(object).map(Object::shape) else {
            return;
        };
        let Some(slot) = self.property_shape_slot(shape, key) else {
            return;
        };
        let mut next = self.shapes[shape as usize].clone();
        next.keys.retain(|candidate| *candidate != key);
        next.slots.remove(&key);
        next.descriptors[slot] = DEFAULT_PROPERTY_ATTRIBUTES;
        let next_id = self.shapes.len() as u32;
        self.heap.register_property_shape(next_id, next.storage_len);
        self.shapes.push(next);
        self.object_data_mut(object)
            .expect("object survived property deletion")
            .set_shape(next_id);
        self.invalidate_method_caches_for_key(key);
    }
}
