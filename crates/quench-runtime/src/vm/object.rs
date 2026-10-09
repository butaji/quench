use super::property_key::PropertyKey;
use super::*;

const PRIVATE_NAME_PREFIX: &str = "\0quench:private:";

/// Bit set of facts derived from an atom's text; `DERIVED` marks the set as computed.
#[derive(Clone, Copy)]
pub(super) struct AtomClass(u8);

impl AtomClass {
    const DERIVED: u8 = 1 << 0;
    pub(super) const PRIVATE: u8 = 1 << 1;
    pub(super) const ARRAY_INDEX: u8 = 1 << 2;
    pub(super) const TYPED_ARRAY_INDEX: u8 = 1 << 3;
    pub(super) const RESTRICTED_FUNCTION_PROPERTY: u8 = 1 << 4;

    #[inline(always)]
    pub(super) fn contains(self, bits: u8) -> bool {
        self.0 & bits == bits
    }
}
pub(super) const FIELD_CACHE_SLOT_CAPACITY: usize = u16::MAX as usize + 1;

struct FieldCacheHit {
    value: Value,
    tier: u8,
    depth: u16,
}

enum FieldCacheRead {
    Proxy,
    Generic,
    StringGeneric,
    Miss(usize),
    StringMiss { prototype: Value, site: usize },
    Hit(FieldCacheHit),
}

fn derive_shape_lookup_index(shapes: &[Shape], shape: u32) -> ShapeLookupIndex {
    let mut entries = Vec::new();
    let mut seen_keys = FxHashSet::default();
    let mut seen_slots = FxHashSet::default();
    let mut attributes = FxHashMap::default();
    let mut current = Some(shape);
    while let Some(id) = current {
        let shape = &shapes[id as usize];
        match shape.transition {
            ShapeTransition::Add { key, slot } => {
                if seen_keys.insert(key) {
                    entries.push((key, slot));
                }
                seen_slots.insert(slot);
            }
            ShapeTransition::Delete { key, slot } => {
                seen_keys.insert(key);
                seen_slots.insert(slot);
            }
            ShapeTransition::Descriptor {
                slot,
                attributes: value,
            } => {
                if seen_slots.insert(slot) && value != DEFAULT_PROPERTY_ATTRIBUTES {
                    attributes.insert(slot, value);
                }
            }
            ShapeTransition::Root
            | ShapeTransition::Vacant
            | ShapeTransition::Dictionary { .. } => {}
        }
        current = shape.parent;
    }
    entries.reverse();
    let slots = entries.iter().copied().collect();
    ShapeLookupIndex {
        entries,
        slots,
        attributes,
    }
}

impl<H: Host> Vm<H> {
    /// Name-derived property classes, computed from the atom text once per atom.
    pub(super) fn atom_class(&self, atom: Atom) -> AtomClass {
        let slot = &self.atom_classes[atom as usize];
        let known = AtomClass(slot.get());
        if known.contains(AtomClass::DERIVED) {
            return known;
        }
        let name = self.atom_name(atom);
        let mut bits = AtomClass::DERIVED;
        if name.starts_with(PRIVATE_NAME_PREFIX) {
            bits |= AtomClass::PRIVATE;
        }
        if super::object_static::array_index(name).is_some() {
            bits |= AtomClass::ARRAY_INDEX;
        }
        if !matches!(
            Self::typed_array_index_key(name),
            super::object_descriptors::TypedArrayIndexKey::NotCanonical
        ) {
            bits |= AtomClass::TYPED_ARRAY_INDEX;
        }
        if matches!(name, "caller" | "arguments") {
            bits |= AtomClass::RESTRICTED_FUNCTION_PROPERTY;
        }
        slot.set(bits);
        AtomClass(bits)
    }

    #[inline(always)]
    pub(super) fn is_private_name(&self, atom: Atom) -> bool {
        self.atom_class(atom).contains(AtomClass::PRIVATE)
    }

    #[inline(always)]
    pub(super) fn shape_slot(&self, shape: u32, atom: Atom) -> Option<usize> {
        self.property_shape_slot(shape, PropertyKey::string(atom))
    }
    #[inline(always)]
    pub(super) fn property_shape_slot(&self, shape: u32, key: PropertyKey) -> Option<usize> {
        if self.shape_is_dictionary(shape) {
            let index = self.shapes[shape as usize]
                .lookup_index
                .get_or_init(|| Box::new(derive_shape_lookup_index(&self.shapes, shape)));
            return index.slots.get(&key).map(|slot| *slot as usize);
        }
        if let Some(index) = self.shapes[shape as usize].lookup_index.get() {
            return index.slots.get(&key).map(|slot| *slot as usize);
        }
        let mut current = Some(shape);
        while let Some(id) = current {
            let shape = &self.shapes[id as usize];
            match shape.transition {
                ShapeTransition::Add {
                    key: candidate,
                    slot,
                } if candidate == key => {
                    return Some(slot as usize);
                }
                ShapeTransition::Delete { key: candidate, .. } if candidate == key => {
                    return None;
                }
                ShapeTransition::Root
                | ShapeTransition::Add { .. }
                | ShapeTransition::Delete { .. }
                | ShapeTransition::Vacant
                | ShapeTransition::Descriptor { .. }
                | ShapeTransition::Dictionary { .. } => current = shape.parent,
            }
        }
        None
    }
    pub(super) fn object_property_slot(
        &self,
        object: Value,
        key: PropertyKey,
    ) -> Option<(u32, usize)> {
        let shape = self.object_data(object)?.shape();
        Some((shape, self.property_shape_slot(shape, key)?))
    }
    fn shape_attribute(&self, shape: u32, slot: usize) -> Option<PropertyAttributes> {
        if let Some(index) = self.shapes[shape as usize].lookup_index.get() {
            return Some(
                index
                    .attributes
                    .get(&(slot as u32))
                    .copied()
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES),
            );
        }
        let mut current = Some(shape);
        while let Some(id) = current {
            let shape = &self.shapes[id as usize];
            match shape.transition {
                ShapeTransition::Descriptor {
                    slot: candidate,
                    attributes,
                } if candidate as usize == slot => return Some(attributes),
                ShapeTransition::Add {
                    slot: candidate, ..
                } if candidate as usize == slot => return Some(DEFAULT_PROPERTY_ATTRIBUTES),
                ShapeTransition::Delete {
                    slot: candidate, ..
                } if candidate as usize == slot => return Some(DEFAULT_PROPERTY_ATTRIBUTES),
                ShapeTransition::Root
                | ShapeTransition::Add { .. }
                | ShapeTransition::Delete { .. }
                | ShapeTransition::Vacant
                | ShapeTransition::Descriptor { .. }
                | ShapeTransition::Dictionary { .. } => current = shape.parent,
            }
        }
        None
    }
    pub(super) fn shape_keys(&self, shape: u32) -> Vec<PropertyKey> {
        self.shape_entries(shape)
            .iter()
            .map(|(key, _)| *key)
            .collect()
    }
    pub(super) fn shape_entries(&self, shape: u32) -> &[(PropertyKey, u32)] {
        &self.shapes[shape as usize]
            .lookup_index
            .get_or_init(|| Box::new(derive_shape_lookup_index(&self.shapes, shape)))
            .entries
    }
    fn append_shape(
        &mut self,
        parent: u32,
        transition: ShapeTransition,
        storage_len: usize,
    ) -> u32 {
        let next = u32::try_from(self.shapes.len()).expect("object shape table exhausted");
        let dictionary_trigger = match transition {
            ShapeTransition::Dictionary { trigger } => Some(trigger),
            _ => self.shapes[parent as usize].dictionary_trigger,
        };
        self.shapes.push(Shape::child(
            Some(parent),
            transition,
            storage_len,
            dictionary_trigger,
        ));
        self.heap.register_property_shape(next, storage_len);
        next
    }
    pub(super) fn shape_is_dictionary(&self, shape: u32) -> bool {
        self.shapes[shape as usize].dictionary_trigger.is_some()
    }
    pub(super) fn mark_object_dictionary(&mut self, object: Value, trigger: DictionaryTrigger) {
        let Some(shape) = self.object_data(object).map(Object::shape) else {
            return;
        };
        if self.shape_is_dictionary(shape) {
            return;
        }
        let storage_len = self.shapes[shape as usize].storage_len;
        let dictionary_shape =
            self.append_shape(shape, ShapeTransition::Dictionary { trigger }, storage_len);
        self.object_data_mut(object)
            .expect("object survived dictionary transition")
            .set_shape(dictionary_shape);
        self.profile.dictionary_transition(trigger);
    }
    #[inline(always)]
    pub(super) fn property_attributes(
        &self,
        object: Value,
        key: PropertyKey,
    ) -> Option<PropertyAttributes> {
        if let Some((shape, slot)) = self.object_property_slot(object, key)
            && let Some(attributes) = self.shape_attribute(shape, slot)
        {
            return Some(attributes);
        }
        // Non-shape properties, including array length and indexed elements,
        // keep explicit attributes in descriptors. Array length always exists;
        // its unchanged attributes derive from the array exotic contract.
        self.descriptors.get(&(object, key)).copied().or_else(|| {
            (key == PropertyKey::string(self.length_atom)
                && self.own_array_length(object).is_some())
            .then_some(super::object_array::ARRAY_LENGTH_ATTRIBUTES)
        })
    }
    pub(super) fn set_property_attributes(
        &mut self,
        object: Value,
        key: PropertyKey,
        attributes: PropertyAttributes,
    ) {
        if let Some((shape, slot)) = self.object_property_slot(object, key) {
            if self.shape_attribute(shape, slot) == Some(attributes) {
                return;
            }
            let next_id = self.append_cached_shape_transition(
                shape,
                ShapeTransition::Descriptor {
                    slot: slot as u32,
                    attributes,
                },
            );
            self.object_data_mut(object)
                .expect("object survived descriptor transition")
                .set_shape(next_id);
            self.invalidate_method_caches_for_key(key);
            return;
        }
        self.descriptors.insert((object, key), attributes);
        self.invalidate_field_caches();
        self.invalidate_method_caches_for_key(key);
    }
    pub(super) fn remove_property_attributes(&mut self, object: Value, key: PropertyKey) {
        if let Some((shape, slot)) = self.object_property_slot(object, key) {
            let next_id = self.append_cached_shape_transition(
                shape,
                ShapeTransition::Descriptor {
                    slot: slot as u32,
                    attributes: DEFAULT_PROPERTY_ATTRIBUTES,
                },
            );
            self.object_data_mut(object)
                .expect("object survived descriptor transition")
                .set_shape(next_id);
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
        self.field_add_caches.clear();
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
        let atom = self.runtime_atoms.lexical_this;
        let value = self
            .dynamic_binding(frame, atom)
            .unwrap_or(self.frames[frame].this);
        self.frames[frame].this = value;
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
    pub(super) fn get(&self, receiver: u32, holder: u32) -> Option<FieldCache> {
        let key = field_cache_key(receiver, holder);
        if let Some(entries) = &self.overflow {
            return entries.get(&key).copied();
        }
        self.entries[..usize::from(self.len)]
            .iter()
            .find(|entry| entry.receiver == receiver && entry.holder == holder)
            .copied()
    }
    fn insert(&mut self, cache: FieldCache) {
        let key = field_cache_key(cache.receiver, cache.holder);
        if let Some(entries) = &mut self.overflow {
            if entries.len() < FIELD_MEGAMORPHIC_LIMIT || entries.contains_key(&key) {
                entries.insert(key, cache);
            }
            return;
        }
        if let Some(entry) = self.entries[..usize::from(self.len)]
            .iter_mut()
            .find(|entry| entry.receiver == cache.receiver && entry.holder == cache.holder)
        {
            *entry = cache;
        } else if usize::from(self.len) < FIELD_MEGAMORPHIC_INLINE {
            self.entries[usize::from(self.len)] = cache;
            self.len += 1;
        } else {
            let mut entries = FxHashMap::default();
            entries.extend(
                self.entries
                    .iter()
                    .map(|entry| (field_cache_key(entry.receiver, entry.holder), *entry)),
            );
            entries.insert(key, cache);
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

#[inline(always)]
fn field_cache_key(receiver: u32, holder: u32) -> u64 {
    (u64::from(receiver) << u32::BITS) | u64::from(holder)
}
impl<H: Host> Vm<H> {
    pub(super) fn object_pair(
        &mut self,
        program: &ResidualProgram,
        site: usize,
        first: Value,
        second: Value,
    ) -> Value {
        if !self.specialized || !program.specialized {
            let [first_atom, second_atom] = program.object_sites[site].atoms;
            let one = self.transition_shape(0, first_atom);
            let two = self.transition_shape(one, second_atom);
            return self
                .heap
                .alloc_object_pair(self.object_proto, two, first, second);
        }
        let cache_site = self.object_cache_index(site);
        let shape = if self.object_shapes[cache_site] != u32::MAX {
            self.object_shapes[cache_site]
        } else {
            let atoms = program.object_sites[site].atoms;
            let one = self.transition_shape(0, atoms[0]);
            let two = self.transition_shape(one, atoms[1]);
            self.object_shapes[cache_site] = two;
            two
        };
        self.heap
            .alloc_object_pair(self.object_proto, shape, first, second)
    }
    pub(super) fn own_property(&self, object: Value, atom: Atom) -> Option<Value> {
        if atom == self.length_atom
            && let Some(length) = self.own_array_length(object)
        {
            return Some(Value::number(length as f64));
        }
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
                    | Cell::ShadowRealm { .. }
                    | Cell::WeakMap { .. }
                    | Cell::WeakSet { .. }
                    | Cell::WeakRef { .. }
                    | Cell::FinalizationRegistry { .. }
                    | Cell::Iterator { .. }
                    | Cell::Proxy { .. }
                    | Cell::Function { .. }
                    | Cell::Date { .. }
                    | Cell::TemporalDuration { .. }
                    | Cell::TemporalPlainDate { .. }
                    | Cell::TemporalPlainDateTime { .. }
                    | Cell::TemporalPlainMonthDay { .. }
                    | Cell::TemporalPlainYearMonth { .. }
                    | Cell::TemporalZonedDateTime { .. }
                    | Cell::TemporalInstant { .. }
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
        let lookup = match self.heap.get(object) {
            Some(Cell::String(_)) if self.specialized && p.specialized => {
                if !self.field_cache_atom_eligible(atom) {
                    FieldCacheRead::StringGeneric
                } else {
                    let prototype = self.string_proto;
                    if let Some(receiver) = self.shape_property_lookup(prototype, atom) {
                        let site = self.field_cache_index(site);
                        match self.cached_field_value(site, receiver) {
                            Some(hit) => FieldCacheRead::Hit(hit),
                            None => FieldCacheRead::StringMiss { prototype, site },
                        }
                    } else {
                        FieldCacheRead::StringGeneric
                    }
                }
            }
            Some(Cell::Proxy { .. }) => FieldCacheRead::Proxy,
            Some(cell) if self.specialized && p.specialized => {
                let Some(receiver) = self.shape_property_lookup_cell(cell, atom) else {
                    return self.get_property(p, object, atom);
                };
                let site = self.field_cache_index(site);
                match self
                    .cached_field_value(site, receiver)
                    .or_else(|| self.cached_holder_field_value(site, receiver, atom))
                {
                    Some(hit) => FieldCacheRead::Hit(hit),
                    None => FieldCacheRead::Miss(site),
                }
            }
            Some(_) | None => FieldCacheRead::Generic,
        };
        match lookup {
            FieldCacheRead::Proxy if self.is_private_name(atom) => {
                self.get_private_proxy_field(p, object, atom)
            }
            FieldCacheRead::Proxy | FieldCacheRead::Generic => self.get_property(p, object, atom),
            FieldCacheRead::StringGeneric => self.get_property(p, object, atom),
            FieldCacheRead::Miss(site) => {
                self.profile.field_cache(false);
                self.get_field_miss(p, object, atom, site)
            }
            FieldCacheRead::StringMiss { prototype, site } => {
                self.profile.field_cache(false);
                self.get_string_field_miss(p, object, prototype, atom, site)
            }
            FieldCacheRead::Hit(hit) => {
                self.profile
                    .field_cache_hit(usize::from(hit.tier), hit.depth);
                Ok(hit.value)
            }
        }
    }

    #[inline(always)]
    fn cached_field_value(&self, site: usize, receiver: &Object) -> Option<FieldCacheHit> {
        let receiver_shape = receiver.shape();
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = unsafe { *self.field_caches.get_unchecked(site) };
        if cache.receiver == receiver_shape && cache.holder == NO_FIELD_HOLDER {
            // SAFETY: receiver shape and slot were recorded together for an
            // own data property on the cache miss path.
            let value = unsafe {
                self.heap
                    .property_get_unchecked(receiver, cache.slot as usize)
            };
            return Some(FieldCacheHit {
                value,
                tier: 0,
                depth: 0,
            });
        }
        if let Some(cache) = self.megamorphic_field_cache(site, receiver_shape, NO_FIELD_HOLDER) {
            // SAFETY: the table is keyed by the immutable receiver shape.
            let value = unsafe {
                self.heap
                    .property_get_unchecked(receiver, cache.slot as usize)
            };
            return Some(FieldCacheHit {
                value,
                tier: 2,
                depth: 0,
            });
        }
        None
    }

    #[inline(always)]
    fn cached_holder_field_value(
        &self,
        site: usize,
        receiver: &Object,
        atom: Atom,
    ) -> Option<FieldCacheHit> {
        let holder_value = receiver.proto;
        let holder_cell = self.heap.get(holder_value)?;
        let holder = self.shape_property_lookup_cell(holder_cell, atom)?;
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = unsafe { *self.field_caches.get_unchecked(site) };
        let (cache, tier) = if cache.receiver == receiver.shape() && cache.holder == holder.shape()
        {
            (cache, 0)
        } else if let Some(cache) =
            self.megamorphic_field_cache(site, receiver.shape(), holder.shape())
        {
            (cache, 2)
        } else {
            return None;
        };
        let value = unsafe {
            // SAFETY: both immutable shapes and the property slot were recorded
            // together after confirming an immediate-prototype data property.
            self.heap
                .property_get_unchecked(holder, cache.slot as usize)
        };
        Some(FieldCacheHit {
            value,
            tier,
            depth: 1,
        })
    }

    #[cold]
    #[inline(never)]
    fn get_string_field_miss(
        &mut self,
        p: &ResidualProgram,
        primitive: Value,
        prototype: Value,
        atom: Atom,
        site: usize,
    ) -> Result<Value, JsError> {
        if self
            .property_attributes(prototype, PropertyKey::string(atom))
            .is_some_and(|attributes| attributes.accessor)
        {
            return self.get_property_with_receiver(p, prototype, atom, primitive);
        }
        let Some(receiver) = self.shape_property_lookup(prototype, atom) else {
            return self.get_property(p, primitive, atom);
        };
        if let Some(slot) = self.shape_slot(receiver.shape(), atom)
            && let Some(value) = self.heap.property_get(receiver, slot)
        {
            if slot < FIELD_CACHE_SLOT_CAPACITY {
                self.record_field_cache(
                    site,
                    FieldCache {
                        receiver: receiver.shape(),
                        holder: NO_FIELD_HOLDER,
                        slot: slot as u16,
                    },
                );
            }
            return Ok(value);
        }
        self.get_property(p, primitive, atom)
    }

    #[cold]
    #[inline(never)]
    pub(super) fn get_field_miss(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: usize,
    ) -> Result<Value, JsError> {
        let Some(receiver) = self.shape_property_lookup(object, atom) else {
            return self.get_property(p, object, atom);
        };
        if self
            .property_attributes(object, PropertyKey::string(atom))
            .is_some_and(|attributes| attributes.accessor)
        {
            return self.get_property(p, object, atom);
        }
        if let Some(slot) = self.shape_slot(receiver.shape(), atom)
            && let Some(value) = self.heap.property_get(receiver, slot)
        {
            if slot < FIELD_CACHE_SLOT_CAPACITY {
                self.record_field_cache(
                    site,
                    FieldCache {
                        receiver: receiver.shape(),
                        holder: NO_FIELD_HOLDER,
                        slot: slot as u16,
                    },
                );
            }
            return Ok(value);
        }
        if let Some((holder_shape, slot, value)) = self.immediate_prototype_data_field(object, atom)
            && slot < FIELD_CACHE_SLOT_CAPACITY
        {
            self.record_field_cache(
                site,
                FieldCache {
                    receiver: receiver.shape(),
                    holder: holder_shape,
                    slot: slot as u16,
                },
            );
            return Ok(value);
        }
        self.get_property(p, object, atom)
    }

    #[cold]
    #[inline(never)]
    fn immediate_prototype_data_field(
        &self,
        receiver_value: Value,
        atom: Atom,
    ) -> Option<(u32, usize, Value)> {
        let holder_value = self.object_data(receiver_value)?.proto;
        let holder_cell = self.heap.get(holder_value)?;
        let holder = self.shape_property_lookup_cell(holder_cell, atom)?;
        let holder_shape = holder.shape();
        let slot = self.shape_slot(holder_shape, atom)?;
        if self
            .shape_attribute(holder_shape, slot)
            .is_some_and(|attributes| attributes.accessor)
        {
            return None;
        }
        Some((holder_shape, slot, self.heap.property_get(holder, slot)?))
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
        let _stack = self.enter_stack()?;
        if self.is_private_name(atom) {
            self.check_private_brand(p, target, atom)?;
        }
        let typed_array_index = if matches!(self.heap.get(target), Some(Cell::TypedArray { .. })) {
            match Self::typed_array_index_key(self.atom_name(atom)) {
                super::object_descriptors::TypedArrayIndexKey::Index(index) => {
                    if target == receiver {
                        return self.typed_array_set(p, target, index, value);
                    }
                    if self
                        .typed_array_length(target)
                        .is_some_and(|length| index < length)
                    {
                        Some(index)
                    } else {
                        return Ok(true);
                    }
                }
                super::object_descriptors::TypedArrayIndexKey::Invalid => {
                    if target == receiver
                        && let Some(kind) = self.typed_array_kind(target)
                    {
                        self.typed_array_convert_value(p, kind, value)?;
                    }
                    return Ok(true);
                }
                super::object_descriptors::TypedArrayIndexKey::NotCanonical => None,
            }
        } else {
            None
        };
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(target).cloned()
        {
            return self.proxy_set(
                p,
                target,
                handler,
                receiver,
                PropertyKey::string(atom),
                value,
            );
        }
        self.evaluate_deferred_namespace_for_key(p, target, Some(PropertyKey::string(atom)))?;

        let mut current = target;
        let mut found = typed_array_index.map(|_| (target, DEFAULT_PROPERTY_ATTRIBUTES));
        while found.is_none() {
            if matches!(self.heap.get(current), Some(Cell::TypedArray { .. })) {
                match Self::typed_array_index_key(self.atom_name(atom)) {
                    super::object_descriptors::TypedArrayIndexKey::Index(index)
                        if current == receiver =>
                    {
                        return self.typed_array_set(p, current, index, value);
                    }
                    super::object_descriptors::TypedArrayIndexKey::Invalid
                        if current == receiver =>
                    {
                        if let Some(kind) = self.typed_array_kind(current) {
                            self.typed_array_convert_value(p, kind, value)?;
                        }
                        return Ok(true);
                    }
                    super::object_descriptors::TypedArrayIndexKey::Index(index)
                        if self
                            .typed_array_length(current)
                            .is_some_and(|length| index < length) =>
                    {
                        found = Some((current, DEFAULT_PROPERTY_ATTRIBUTES));
                        break;
                    }
                    super::object_descriptors::TypedArrayIndexKey::Index(_)
                    | super::object_descriptors::TypedArrayIndexKey::Invalid => {
                        return Ok(true);
                    }
                    super::object_descriptors::TypedArrayIndexKey::NotCanonical => {}
                }
            }
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = self.heap.get(current).cloned()
            {
                return self.proxy_set(
                    p,
                    target,
                    handler,
                    receiver,
                    PropertyKey::string(atom),
                    value,
                );
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

        if !self.is_object_like(receiver) {
            return Ok(false);
        }
        if matches!(self.heap.get(receiver), Some(Cell::TypedArray { .. })) {
            match Self::typed_array_index_key(self.atom_name(atom)) {
                super::object_descriptors::TypedArrayIndexKey::Index(index)
                    if self
                        .typed_array_length(receiver)
                        .is_some_and(|length| index < length) =>
                {
                    return self.typed_array_set(p, receiver, index, value);
                }
                super::object_descriptors::TypedArrayIndexKey::Index(_)
                | super::object_descriptors::TypedArrayIndexKey::Invalid => return Ok(false),
                super::object_descriptors::TypedArrayIndexKey::NotCanonical => {}
            }
        }
        if matches!(self.heap.get(receiver), Some(Cell::Proxy { .. })) {
            return self.set_receiver_proxy_data_property(
                p,
                receiver,
                PropertyKey::string(atom),
                value,
            );
        }
        let new_property = if target == receiver {
            found.is_none_or(|(owner, _)| owner != receiver)
        } else {
            self.own_property(receiver, atom).is_none()
        };
        if !new_property
            && let Some(attributes) = self.property_attributes(receiver, PropertyKey::string(atom))
            && (attributes.accessor || !attributes.writable)
        {
            return Ok(false);
        }
        self.define_receiver_data_property(p, receiver, atom, value, new_property)
    }

    fn define_receiver_data_property(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        atom: Atom,
        value: Value,
        new_property: bool,
    ) -> Result<bool, JsError> {
        if let Some(defined) = self.define_receiver_array_data_property(p, receiver, atom, value)? {
            return Ok(defined);
        }
        if new_property
            && !self
                .object_data(receiver)
                .is_some_and(Object::is_extensible)
        {
            return Ok(false);
        }
        if new_property {
            self.create_shape_property(receiver, PropertyKey::string(atom), value)?;
        } else {
            self.set_shape_property(receiver, PropertyKey::string(atom), value)?;
        }
        self.mirror_global_var_property_write(receiver, atom, value);
        Ok(true)
    }

    fn define_receiver_array_data_property(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        atom: Atom,
        value: Value,
    ) -> Result<Option<bool>, JsError> {
        if !matches!(self.heap.get(receiver), Some(Cell::Array { .. })) {
            return Ok(None);
        }
        if atom == self.length_atom
            && !self
                .object_data(receiver)
                .is_some_and(Object::is_arguments_object)
        {
            return self.set_array_length(p, receiver, value).map(Some);
        }
        let Some(index) = super::object_static::array_index(self.atom_name(atom)) else {
            return Ok(None);
        };
        if let Some(attributes) = self.array_descriptor(receiver, index as usize) {
            if attributes.accessor {
                if let Some(setter) = attributes.setter {
                    self.call_value(p, setter, receiver, &[value])?;
                    return Ok(Some(true));
                }
                return Ok(Some(false));
            }
            if !attributes.writable {
                return Ok(Some(false));
            }
        }
        Ok(Some(self.set_array_element(
            receiver,
            index as usize,
            value,
        )))
    }

    pub(super) fn set_receiver_proxy_data_property(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        property: PropertyKey,
        value: Value,
    ) -> Result<bool, JsError> {
        let key = match property {
            PropertyKey::String(atom) => self.heap.alloc(Cell::String(self.atom_value(atom))),
            PropertyKey::Symbol(key) => key,
            PropertyKey::Private(_) => {
                unreachable!("private keys cannot reach Proxy DefineOwnProperty")
            }
        };
        let receiver = self.heap.root(receiver);
        let key = self.heap.root(key);
        let value = self.heap.root(value);
        let outcome = (|| {
            let current = self.object_get_own_property_descriptor(
                p,
                &[
                    self.heap.root_value(receiver).unwrap(),
                    self.heap.root_value(key).unwrap(),
                ],
            )?;
            let descriptor = if current.is_undefined() {
                super::object_descriptors::PropertyDescriptorRecord::data(
                    self.heap.root_value(value).unwrap(),
                )
            } else {
                if self.descriptor_field(p, current, "get")?.is_some()
                    || self.descriptor_field(p, current, "set")?.is_some()
                    || !self.descriptor_flag(current, "writable")
                {
                    return Ok(false);
                }
                super::object_descriptors::PropertyDescriptorRecord::value(
                    self.heap.root_value(value).unwrap(),
                )
            };
            self.define_own_property_record(
                p,
                self.heap.root_value(receiver).unwrap(),
                self.heap.root_value(key).unwrap(),
                descriptor,
            )
        })();
        for root in [receiver, key, value] {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn set_shape_property(
        &mut self,
        object: Value,
        key: PropertyKey,
        value: Value,
    ) -> Result<(), JsError> {
        let (slot, exists) = {
            let data = self
                .object_data(object)
                .ok_or_else(|| JsError("property write on non-object".into()))?;
            let slot = self.property_shape_slot(data.shape(), key);
            let exists = slot.is_some_and(|slot| self.heap.property_get(data, slot).is_some());
            (slot, exists)
        };
        self.check_property_key_write(object, key, exists)?;
        let invalidates_method = slot.is_some_and(|slot| self.callable_write(object, slot, value));
        if let Some(slot) = slot {
            self.heap.property_set(object, slot, value);
        } else {
            return self.create_shape_property(object, key, value);
        }
        if invalidates_method {
            self.invalidate_method_caches_for_key(key);
        }
        Ok(())
    }

    /// Create a shape-backed property after proving its absence.
    fn create_shape_property(
        &mut self,
        object: Value,
        key: PropertyKey,
        value: Value,
    ) -> Result<(), JsError> {
        let shape = self
            .object_data(object)
            .ok_or_else(|| JsError("property write on non-object".into()))?
            .shape();
        self.check_property_key_write(object, key, false)?;
        let next_shape = self.transition_property_shape(shape, key);
        self.heap.property_push(object, value);
        self.object_data_mut(object).unwrap().set_shape(next_shape);
        if let PropertyKey::String(atom) = key {
            self.invalidate_method_caches_for_prototype_add(object, atom);
        }
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
            if self.is_private_name(atom) {
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
            let written =
                self.proxy_set(p, target, handler, object, PropertyKey::string(atom), value)?;
            return if written || !strict {
                Ok(())
            } else {
                Err(self.type_error(p, "cannot assign property through proxy".into()))
            };
        }
        if !self.is_object_like(object) {
            if self.is_private_name(atom) {
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
        if self.specialized
            && p.specialized
            && !self.is_private_name(atom)
            && let Some(Cell::Object(data)) = self.heap.get(object)
            && !data.is_module_namespace()
            && !data.is_arguments_object()
        {
            let site = self.field_cache_index(site);
            let shape = data.shape();
            if self.try_cached_field_store(object, atom, shape, value, site)
                || (self.field_cache_atom_eligible(atom)
                    && self.try_cached_field_add(object, atom, shape, value, site))
            {
                return Ok(());
            }
        }
        let own = self.own_property(object, atom).is_some();
        if !own && self.prototype_chain_contains_proxy(object) {
            return self.set_property_with_program_mode(p, object, atom, value, strict);
        }
        if let Some(attributes) = self.property_accessor(object, atom) {
            if let Some(setter) = attributes.setter {
                self.call_value(p, setter, object, &[value])?;
            } else if strict || self.is_private_name(atom) {
                return Err(self.type_error(p, "property has no setter".into()));
            }
            return Ok(());
        }
        if own
            && self
                .property_attributes(object, PropertyKey::string(atom))
                .is_some_and(|attributes| !attributes.writable)
        {
            return if strict || self.is_private_name(atom) {
                Err(self.type_error(p, "cannot write non-writable property".into()))
            } else {
                Ok(())
            };
        }
        if !own && self.inherited_write_blocked(object, atom) {
            return if strict || self.is_private_name(atom) {
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
        if self.is_private_name(atom) {
            self.set_property(object, atom, value)?;
            return Ok(());
        }
        if !self.specialized || !p.specialized {
            return self.set_property_with_program(p, object, atom, value);
        }
        let site = self.field_cache_index(site);
        let existing = self
            .object_data(object)
            .and_then(|data| self.shape_slot(data.shape(), atom));
        self.check_property_write(object, atom, existing.is_some())?;
        let shape = self
            .object_data(object)
            .map(Object::shape)
            .unwrap_or(u32::MAX);
        let add_prototype_shapes =
            if existing.is_none() && shape != u32::MAX && self.field_cache_atom_eligible(atom) {
                self.field_add_prototype_shapes(object, atom)
            } else {
                None
            };
        if shape != u32::MAX && self.try_cached_field_store(object, atom, shape, value, site) {
            return Ok(());
        }
        self.profile.field_cache(false);
        self.set_property(object, atom, value)?;
        let data = self.object_data(object).unwrap();
        let slot = self
            .shape_slot(data.shape(), atom)
            .expect("property transition records the new shape slot");
        let data_shape = data.shape();
        if slot <= u16::MAX as usize && !self.shape_is_dictionary(data_shape) {
            if existing.is_none()
                && data_shape != shape
                && slot == self.shapes[shape as usize].storage_len
                && !self.shape_is_dictionary(shape)
                && let Some(prototype_shapes) = add_prototype_shapes
            {
                self.field_add_caches.insert(
                    site,
                    FieldAddCache {
                        atom,
                        source_shape: shape,
                        target_shape: data_shape,
                        slot: slot as u16,
                        prototype_shapes,
                    },
                );
            }
            self.record_field_cache(
                site,
                FieldCache {
                    receiver: data_shape,
                    holder: NO_FIELD_HOLDER,
                    slot: slot as u16,
                },
            );
        }
        Ok(())
    }

    fn try_cached_field_add(
        &mut self,
        object: Value,
        atom: Atom,
        shape: u32,
        value: Value,
        site: usize,
    ) -> bool {
        let Some(cache) = self.field_add_caches.get(&site) else {
            return false;
        };
        if cache.atom != atom
            || cache.source_shape != shape
            || self.shape_is_dictionary(shape)
            || !self.object_data(object).is_some_and(Object::is_extensible)
            || !self.field_add_prototype_chain_matches(object, &cache.prototype_shapes)
        {
            return false;
        }
        let target_shape = cache.target_shape;
        let slot = cache.slot;
        self.heap.property_push(object, value);
        self.object_data_mut(object)
            .expect("field-add cache requires an ordinary object")
            .set_shape(target_shape);
        self.invalidate_method_caches_for_prototype_add(object, atom);
        self.profile.field_cache_hit(0, 0);
        debug_assert_eq!(slot as usize, self.shapes[shape as usize].storage_len);
        true
    }

    fn field_add_prototype_shapes(&self, object: Value, atom: Atom) -> Option<Vec<u32>> {
        let mut prototype = self.object_data(object)?.proto;
        let key = PropertyKey::string(atom);
        let mut shapes = Vec::new();
        while !prototype.is_null() {
            if shapes.len() == FIELD_ADD_CACHE_MAX_PROTO_DEPTH {
                return None;
            }
            let Some(Cell::Object(data)) = self.heap.get(prototype) else {
                return None;
            };
            let shape = data.shape();
            if data.is_module_namespace()
                || data.is_arguments_object()
                || self.shape_is_dictionary(shape)
                || self
                    .property_attributes(prototype, key)
                    .is_some_and(|attributes| attributes.accessor || !attributes.writable)
            {
                return None;
            }
            shapes.push(shape);
            prototype = data.proto;
        }
        Some(shapes)
    }

    fn field_add_prototype_chain_matches(&self, object: Value, expected: &[u32]) -> bool {
        let Some(mut prototype) = self.object_data(object).map(|data| data.proto) else {
            return false;
        };
        for expected_shape in expected {
            let Some(Cell::Object(data)) = self.heap.get(prototype) else {
                return false;
            };
            let shape = data.shape();
            if shape != *expected_shape
                || data.is_module_namespace()
                || data.is_arguments_object()
                || self.shape_is_dictionary(shape)
            {
                return false;
            }
            prototype = data.proto;
        }
        prototype.is_null()
    }

    #[inline(always)]
    fn try_cached_field_store(
        &mut self,
        object: Value,
        atom: Atom,
        shape: u32,
        value: Value,
        site: usize,
    ) -> bool {
        let cache = self.field_caches[site];
        let (cache, kind) = if cache.receiver == shape && cache.holder == NO_FIELD_HOLDER {
            (cache, 0)
        } else if let Some(cache) = self.megamorphic_field_cache(site, shape, NO_FIELD_HOLDER) {
            (cache, 2)
        } else {
            return false;
        };
        let invalidates_method = self.callable_write(object, cache.slot as usize, value);
        // SAFETY: a matching immutable shape proves the cached slot layout.
        unsafe {
            self.heap
                .property_set_unchecked(object, cache.slot as usize, value);
        }
        if invalidates_method {
            self.invalidate_method_caches_for_atom(atom);
        }
        self.profile.field_cache_hit(kind, 0);
        true
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
        if !self.is_object_like(object) {
            if self.is_private_name(atom) {
                self.check_private_brand(p, object, atom)?;
            }
            let boxed = self.box_primitive_object(object)?;
            let written = self.set_property_with_receiver(p, boxed, atom, value, object)?;
            return if written || !strict {
                Ok(())
            } else {
                Err(self.type_error(p, "cannot assign property on primitive value".into()))
            };
        }
        let written = self.set_property_with_receiver(p, object, atom, value, object)?;
        if written || !strict {
            Ok(())
        } else {
            Err(self.type_error(p, "cannot assign property".into()))
        }
    }

    pub(super) fn prototype_chain_contains_proxy(&self, object: Value) -> bool {
        let mut current = self.object_data(object).map(|data| data.proto);
        while let Some(value) = current.filter(|value| !value.is_null()) {
            if matches!(self.heap.get(value), Some(Cell::Proxy { .. })) {
                return true;
            }
            current = self.object_data(value).map(|data| data.proto);
        }
        false
    }

    pub(super) fn prototype_chain_has_typed_array(&self, object: Value) -> bool {
        let mut current = self.object_data(object).map(|data| data.proto);
        while let Some(value) = current.filter(|value| !value.is_null()) {
            if matches!(self.heap.get(value), Some(Cell::TypedArray { .. })) {
                return true;
            }
            current = self.object_data(value).map(|data| data.proto);
        }
        false
    }

    pub(super) fn prototype_chain_has_indexed_set_exotic(&self, object: Value) -> bool {
        let mut current = self.object_data(object).map(|data| data.proto);
        while let Some(value) = current.filter(|value| !value.is_null()) {
            if matches!(
                self.heap.get(value),
                Some(Cell::Proxy { .. } | Cell::TypedArray { .. })
            ) {
                return true;
            }
            current = self.object_data(value).map(|data| data.proto);
        }
        false
    }

    pub(super) fn property_accessor(
        &self,
        mut object: Value,
        atom: Atom,
    ) -> Option<PropertyAttributes> {
        loop {
            if matches!(self.heap.get(object), Some(Cell::TypedArray { .. }))
                && !matches!(
                    Self::typed_array_index_key(self.atom_name(atom)),
                    super::object_descriptors::TypedArrayIndexKey::NotCanonical
                )
            {
                return None;
            }
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
    pub(super) fn record_field_cache(&mut self, site: usize, cache: FieldCache) {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors.
        let table_index = unsafe { *self.megamorphic_field_indices.get_unchecked(site) };
        if table_index != NO_MEGAMORPHIC_FIELD {
            // SAFETY: every non-sentinel index is written when its table is pushed.
            let entries = unsafe {
                self.megamorphic_fields
                    .get_unchecked_mut(table_index as usize)
            };
            entries.insert(cache);
            return;
        }
        let entry = &mut self.field_caches[site];
        if (entry.receiver == cache.receiver && entry.holder == cache.holder)
            || entry.receiver == NO_FIELD_RECEIVER
        {
            *entry = cache;
            return;
        }
        let table_index = self.megamorphic_fields.len() as u32;
        self.megamorphic_fields
            .push(FieldCacheSet::with_pair(*entry, cache));
        // SAFETY: compiler-produced sites index exactly-sized cache vectors.
        unsafe {
            *self.megamorphic_field_indices.get_unchecked_mut(site) = table_index;
        }
    }
    #[inline(always)]
    fn megamorphic_field_cache(
        &self,
        site: usize,
        receiver: u32,
        holder: u32,
    ) -> Option<FieldCache> {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors;
        // every non-sentinel index was installed together with its table.
        let table_index = unsafe { *self.megamorphic_field_indices.get_unchecked(site) };
        if table_index == NO_MEGAMORPHIC_FIELD {
            return None;
        }
        unsafe {
            self.megamorphic_fields
                .get_unchecked(table_index as usize)
                .get(receiver, holder)
        }
    }
    pub(super) fn transition_shape(&mut self, shape: u32, atom: Atom) -> u32 {
        self.transition_property_shape(shape, PropertyKey::string(atom))
    }
    pub(super) fn transition_property_shape(&mut self, shape: u32, key: PropertyKey) -> u32 {
        let transition = ShapeTransition::Add {
            key,
            slot: u32::try_from(self.shapes[shape as usize].storage_len)
                .expect("object property index exceeds u32"),
        };
        self.append_cached_shape_transition(shape, transition)
    }
    fn append_cached_shape_transition(&mut self, shape: u32, transition: ShapeTransition) -> u32 {
        let key = transition.cache_key();
        if let Some(key) = key
            && let Some(next) = self.transitions.get(&(shape, key)).copied()
        {
            self.profile.shape_transition(true);
            return next;
        }
        if key.is_some() || matches!(transition, ShapeTransition::Add { .. }) {
            self.profile.shape_transition(false);
        }
        let storage_len = self.shapes[shape as usize].storage_len;
        let next_storage_len = match transition {
            ShapeTransition::Add { .. } => storage_len
                .checked_add(1)
                .filter(|length| u32::try_from(*length).is_ok())
                .expect("object property storage exhausted"),
            ShapeTransition::Descriptor { .. } => storage_len,
            ShapeTransition::Root
            | ShapeTransition::Delete { .. }
            | ShapeTransition::Vacant
            | ShapeTransition::Dictionary { .. } => {
                unreachable!("cached shape transitions are adds or descriptors")
            }
        };
        let mut next = self.append_shape(shape, transition, next_storage_len);
        if matches!(transition, ShapeTransition::Add { .. })
            && self.shapes[next as usize].dictionary_trigger.is_none()
            && next_storage_len > FIELD_CACHE_SLOT_CAPACITY
        {
            next = self.append_shape(
                next,
                ShapeTransition::Dictionary {
                    trigger: DictionaryTrigger::PropertyCount,
                },
                next_storage_len,
            );
            self.profile
                .dictionary_transition(DictionaryTrigger::PropertyCount);
        }
        if let Some(key) = key {
            self.transitions.insert((shape, key), next);
        }
        next
    }
    pub(super) fn delete_shape_property(&mut self, object: Value, key: PropertyKey) {
        let Some(shape) = self.object_data(object).map(Object::shape) else {
            return;
        };
        let Some(slot) = self.property_shape_slot(shape, key) else {
            return;
        };
        let storage_len = self.shapes[shape as usize].storage_len;
        let next_id = self.append_shape(
            shape,
            ShapeTransition::Delete {
                key,
                slot: slot as u32,
            },
            storage_len,
        );
        let next_id = if self.shapes[next_id as usize].dictionary_trigger.is_none() {
            let dictionary_shape = self.append_shape(
                next_id,
                ShapeTransition::Dictionary {
                    trigger: DictionaryTrigger::DeletionPattern,
                },
                storage_len,
            );
            self.profile
                .dictionary_transition(DictionaryTrigger::DeletionPattern);
            dictionary_shape
        } else {
            next_id
        };
        self.object_data_mut(object)
            .expect("object survived property deletion")
            .set_shape(next_id);
        self.invalidate_method_caches_for_key(key);
    }
}
