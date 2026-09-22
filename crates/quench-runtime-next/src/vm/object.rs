use super::property_key::PropertyKey;
use super::*;
impl<H: Host> Vm<H> {
    #[inline(always)]
    pub(super) fn shape_slot(&self, shape: u32, atom: Atom) -> Option<usize> {
        self.shapes
            .get(shape as usize)
            .and_then(|shape| shape.slots.get(&atom).copied())
            .map(usize::from)
    }
    #[inline(always)]
    pub(super) fn property_attributes(
        &self,
        object: Value,
        key: PropertyKey,
    ) -> Option<PropertyAttributes> {
        if let PropertyKey::String(atom) = key
            && let Some(data) = self.object_data(object)
            && let Some(slot) = self.shape_slot(data.shape(), atom)
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
        if let PropertyKey::String(atom) = key
            && let Some(data) = self.object_data(object)
            && let Some(slot) = self.shape_slot(data.shape(), atom)
        {
            let shape = data.shape();
            let mut next = self.shapes[shape as usize].clone();
            next.descriptors[slot] = attributes;
            let next_id = self.shapes.len() as u32;
            self.shapes.push(next);
            self.heap
                .register_property_shape(next_id, self.shapes[next_id as usize].keys.len());
            self.object_data_mut(object)
                .expect("object survived descriptor transition")
                .set_shape(next_id);
            self.invalidate_field_caches();
            self.invalidate_method_caches();
            return;
        }
        self.descriptors.insert((object, key), attributes);
    }
    pub(super) fn remove_property_attributes(&mut self, object: Value, key: PropertyKey) {
        if let PropertyKey::String(atom) = key
            && let Some(data) = self.object_data(object)
            && let Some(slot) = self.shape_slot(data.shape(), atom)
        {
            let shape = data.shape();
            let mut next = self.shapes[shape as usize].clone();
            next.descriptors[slot] = DEFAULT_PROPERTY_ATTRIBUTES;
            let next_id = self.shapes.len() as u32;
            self.shapes.push(next);
            self.heap
                .register_property_shape(next_id, self.shapes[next_id as usize].keys.len());
            self.object_data_mut(object)
                .expect("object survived descriptor transition")
                .set_shape(next_id);
            self.invalidate_field_caches();
            self.invalidate_method_caches();
            return;
        }
        self.descriptors.remove(&(object, key));
    }
    pub(super) fn invalidate_field_caches(&mut self) {
        self.field_caches.fill(EMPTY_CACHE);
        self.megamorphic_field_indices.fill(NO_MEGAMORPHIC_FIELD);
        self.megamorphic_fields.clear();
    }
    #[inline(always)]
    pub(super) fn resolve_field_base(&self, frame: usize, base: FieldBase) -> Value {
        match base.register_index() {
            Some(register) => self.read(frame, register),
            None => self.frames[frame].this,
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
    pub(super) fn get_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Result<Value, JsError> {
        if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) {
            return self.get_property(p, object, atom);
        }
        if !self.specialized {
            return self.get_property(p, object, atom);
        }
        if !object.is_heap()
            || atom == self.length_atom
            || atom == self.size_atom
            || self.lookup_atom("byteLength") == Some(atom)
            || self.lookup_atom("byteOffset") == Some(atom)
            || self.lookup_atom("buffer") == Some(atom)
        {
            return self.get_property(p, object, atom);
        }
        if self.property_accessor(object, atom).is_some() {
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
        if self.lookup_atom("byteLength") == Some(atom)
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
        let (slot, shape, invalidates_method) = {
            let data = self
                .object_data(object)
                .ok_or_else(|| JsError("property write on non-object".into()))?;
            let slot = self.shape_slot(data.shape(), atom);
            let exists = slot.is_some_and(|slot| self.heap.property_get(data, slot).is_some());
            self.check_property_write(object, atom, exists)?;
            let old_is_function = slot
                .and_then(|slot| self.heap.property_get(data, slot))
                .is_some_and(|old| self.is_function(old));
            (
                slot,
                data.shape(),
                old_is_function || self.is_function(value),
            )
        };
        let next_shape = slot
            .map(|_| shape)
            .unwrap_or_else(|| self.transition_shape(shape, atom));
        if let Some(slot) = slot {
            self.heap.property_set(object, slot, value);
        } else {
            self.heap.property_push(object, value);
            self.object_data_mut(object).unwrap().set_shape(next_shape);
        }
        // New ordinary properties receive the default descriptor in the
        // transition shape; exotic/indexed properties retain their keyed
        // descriptor path.
        // A property write can replace a callable observed through any
        // receiver/prototype cache. Until mutation epochs are part of the
        // cache key, clear the derived method view at this single mutation
        // boundary; correctness takes precedence over a stale fast path.
        let _ = invalidates_method;
        self.invalidate_field_caches();
        self.invalidate_method_caches();
        Ok(())
    }
    pub(super) fn set_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> Result<(), JsError> {
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(object).cloned()
        {
            return self.proxy_set(p, target, handler, object, atom, value);
        }
        if !self.specialized {
            return self.set_property_with_program(p, object, atom, value);
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
            let _ = invalidates_method;
            self.invalidate_field_caches();
            self.invalidate_method_caches();
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
            let _ = invalidates_method;
            self.invalidate_field_caches();
            self.invalidate_method_caches();
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
        if atom == self.length_atom && matches!(self.heap.get(object), Some(Cell::Array { .. })) {
            let descriptor = self.object();
            let value_atom = self.intern_atom("value");
            self.set_property(descriptor, value_atom, value)?;
            self.define_array_length(p, object, descriptor)?;
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
        self.set_property(object, atom, value)
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
        if let Some(next) = self.transitions.get(&(shape, atom)).copied() {
            self.profile.shape_transition(true);
            return next;
        }
        self.profile.shape_transition(false);
        let mut fields = self.shapes[shape as usize].keys.clone();
        let mut slots = self.shapes[shape as usize].slots.clone();
        let mut descriptors = self.shapes[shape as usize].descriptors.clone();
        slots.insert(atom, fields.len() as u16);
        fields.push(atom);
        descriptors.push(DEFAULT_PROPERTY_ATTRIBUTES);
        let next = self.shapes.len() as u32;
        self.shapes.push(Shape {
            keys: fields,
            slots,
            descriptors,
        });
        self.heap
            .register_property_shape(next, self.shapes[next as usize].keys.len());
        self.transitions.insert((shape, atom), next);
        next
    }
}
