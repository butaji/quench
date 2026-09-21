use super::*;
impl<H: Host> Vm<H> {
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
    fn get(&self, shape: u32) -> Option<FieldCache> {
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
        let slot = self.shapes[object.shape() as usize]
            .iter()
            .position(|key| *key == atom)?;
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
        if !object.is_heap()
            || atom == self.length_atom
            || atom == self.size_atom
            || self.lookup_atom("byteLength") == Some(atom)
            || self.lookup_atom("byteOffset") == Some(atom)
            || self.lookup_atom("buffer") == Some(atom)
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
        if cache.receiver == receiver_shape {
            // SAFETY: the immutable receiver shape and slot were recorded
            // together for an own property on the cache miss path.
            let value = unsafe {
                self.heap
                    .property_get_unchecked(receiver, cache.slot as usize)
            };
            self.profile.field_cache_hit(0, 0);
            return Ok(value);
        }
        if let Some(cache) = self.megamorphic_field_cache(site, receiver_shape) {
            // SAFETY: the table is keyed by the immutable receiver shape.
            let value = unsafe {
                self.heap
                    .property_get_unchecked(receiver, cache.slot as usize)
            };
            self.profile.field_cache_hit(2, 0);
            return Ok(value);
        }
        self.profile.field_cache(false);
        self.get_field_miss(object, atom, site)
    }
    #[cold]
    #[inline(never)]
    pub(super) fn get_field_miss(
        &mut self,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Result<Value, JsError> {
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
        let mut depth = 0u8;
        loop {
            let Some(current) = self.object_data(owner) else {
                return Ok(Value::UNDEFINED);
            };
            if let Some(slot) = self.shapes[current.shape() as usize]
                .iter()
                .position(|key| *key == atom)
            {
                let value = self.heap.property_get(current, slot).unwrap();
                if depth == 0 && slot <= u16::MAX as usize {
                    let receiver = self
                        .object_data(object)
                        .map(Object::shape)
                        .unwrap_or(u32::MAX);
                    self.record_field_cache(
                        site,
                        FieldCache {
                            receiver,
                            slot: slot as u16,
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
    pub(super) fn get_property(
        &self,
        _p: &ResidualProgram,
        mut object: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        if object.as_number().is_some() {
            return Ok(if atom == self.primitive_atoms[4] {
                self.native_value(Native::NumberString)
            } else if atom == self.to_fixed_atom {
                self.native_value(Native::NumberFixed)
            } else if atom == self.to_precision_atom {
                self.native_value(Native::NumberPrecision)
            } else {
                Value::UNDEFINED
            });
        }
        loop {
            if let Some(v) = self.own_property(object, atom) {
                return Ok(v);
            }
            if let Some(v) = self.indexed_view_property(object, atom) {
                return Ok(v);
            }
            match self.heap.get(object) {
                Some(Cell::ArrayBuffer { bytes, shared, .. })
                    if self.lookup_atom("byteLength") == Some(atom) =>
                {
                    let _shared = shared;
                    return Ok(Value::number(if self.array_buffer_detached(object) {
                        0.0
                    } else {
                        bytes.len() as f64
                    }));
                }
                Some(Cell::Array { .. }) if atom == self.length_atom => {
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
                Some(Cell::Uint8Array { object: x, .. })
                | Some(Cell::Uint16Array { object: x, .. })
                | Some(Cell::Uint32Array { object: x, .. })
                | Some(Cell::Int8Array { object: x, .. })
                | Some(Cell::Int16Array { object: x, .. })
                | Some(Cell::Int32Array { object: x, .. }) => object = x.proto,
                Some(Cell::DataView { object: x, .. }) => object = x.proto,
                Some(Cell::Set { entries, .. }) if atom == self.size_atom => {
                    return Ok(Value::number(entries.len() as f64));
                }
                Some(Cell::String(v)) => {
                    return Ok(if atom == self.length_atom {
                        Value::number(v.encode_utf16().count() as f64)
                    } else if atom == self.primitive_atoms[0] {
                        self.native_value(Native::StringCharCodeAt)
                    } else if atom == self.primitive_atoms[1] {
                        self.native_value(Native::StringCharAt)
                    } else if atom == self.primitive_atoms[2] {
                        self.native_value(Native::StringSubstring)
                    } else if atom == self.primitive_atoms[3] {
                        self.native_value(Native::StringSubstr)
                    } else if atom == self.primitive_atoms[5] {
                        self.native_value(Native::StringIncludes)
                    } else if atom == self.primitive_atoms[6] {
                        self.native_value(Native::StringStartsWith)
                    } else if atom == self.primitive_atoms[7] {
                        self.native_value(Native::StringEndsWith)
                    } else {
                        Value::UNDEFINED
                    });
                }
                Some(Cell::Object(x)) | Some(Cell::Array { object: x, .. }) => object = x.proto,
                Some(Cell::Map { object: x, .. }) | Some(Cell::Set { object: x, .. }) => {
                    object = x.proto
                }
                Some(Cell::WeakMap { object: x, .. }) | Some(Cell::WeakSet { object: x, .. }) => {
                    object = x.proto
                }
                Some(Cell::WeakRef { object: x, .. }) => object = x.proto,
                Some(Cell::Iterator { object: x, .. }) => object = x.proto,
                Some(Cell::Function { object: x, .. }) => object = x.proto,
                _ => return Ok(Value::UNDEFINED),
            }
            if object.is_null() {
                return Ok(Value::UNDEFINED);
            }
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
            let slot = self.shapes[data.shape() as usize]
                .iter()
                .position(|key| *key == atom);
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
        if invalidates_method {
            self.invalidate_method_caches();
        }
        Ok(())
    }
    pub(super) fn set_field_cached(
        &mut self,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> Result<(), JsError> {
        let shape = self
            .object_data(object)
            .map(Object::shape)
            .unwrap_or(u32::MAX);
        let cache = self.field_caches[site as usize];
        if shape != u32::MAX && cache.receiver == shape {
            let invalidates_method = self.callable_write(object, cache.slot as usize, value);
            // SAFETY: a matching immutable shape proves the cached slot layout.
            unsafe {
                self.heap
                    .property_set_unchecked(object, cache.slot as usize, value);
            }
            if invalidates_method {
                self.invalidate_method_caches();
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
                self.invalidate_method_caches();
            }
            self.profile.field_cache_hit(2, 0);
            return Ok(());
        }
        self.profile.field_cache(false);
        self.set_property(object, atom, value)?;
        let data = self.object_data(object).unwrap();
        let slot = self.shapes[data.shape() as usize]
            .iter()
            .position(|key| *key == atom)
            .unwrap();
        let data_shape = data.shape();
        if slot <= u16::MAX as usize {
            self.record_field_cache(
                site,
                FieldCache {
                    receiver: data_shape,
                    slot: slot as u16,
                },
            );
        }
        Ok(())
    }
    fn callable_write(&self, object: Value, slot: usize, value: Value) -> bool {
        self.is_function(value)
            || self
                .object_data(object)
                .and_then(|object| self.heap.property_get(object, slot))
                .is_some_and(|old| self.is_function(old))
    }
    fn record_field_cache(&mut self, site: u16, cache: FieldCache) {
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
        let mut fields = self.shapes[shape as usize].clone();
        fields.push(atom);
        let next = self.shapes.len() as u32;
        self.shapes.push(fields);
        self.heap
            .register_property_shape(next, self.shapes[next as usize].len());
        self.transitions.insert((shape, atom), next);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SilentHost;
    impl Host for SilentHost {
        fn write_line(&mut self, _: &str) {}
        fn clock_millis(&mut self) -> f64 {
            0.0
        }
    }

    #[test]
    fn third_receiver_promotes_field_site_to_megamorphic() {
        let mut vm = Vm::new(SilentHost);
        vm.field_caches.push(EMPTY_CACHE);
        vm.megamorphic_field_indices.push(NO_MEGAMORPHIC_FIELD);
        for receiver in 1..=4 {
            vm.record_field_cache(0, FieldCache { receiver, slot: 0 });
        }
        let table = &vm.megamorphic_fields[0];
        assert_eq!(table.len(), 4);
        assert!(table.get(1).is_some() && table.get(4).is_some());
        assert_eq!(vm.megamorphic_field_indices, [0]);
    }
}
