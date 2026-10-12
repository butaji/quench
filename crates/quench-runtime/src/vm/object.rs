use super::property_key::PropertyKey;
use super::*;
use crate::value::HeapReference;

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
    /// Names that exotic or accessor-backed built-in receivers (array `length`, collection
    /// `size`, buffer and view geometry) answer outside ordinary shape storage, so field
    /// caches never record them.
    pub(super) const VIRTUAL_FIELD: u8 = 1 << 5;
    /// The compiler's hidden name for a named function expression's own immutable binding.
    pub(super) const SELF_BINDING: u8 = 1 << 6;
    /// Engine-internal names, which begin with NUL and never name a source binding.
    pub(super) const INTERNAL: u8 = 1 << 7;

    #[inline(always)]
    pub(super) fn contains(self, bits: u8) -> bool {
        self.0 & bits == bits
    }

    #[inline(always)]
    pub(super) fn intersects(self, bits: u8) -> bool {
        self.0 & bits != 0
    }
}
/// Transitions a shape lookup may walk before the walk counts as long.
const SHAPE_INDEX_WALK_DISTANCE: usize = 32;
/// Long walks from one shape after which that shape gets a lookup index.
const SHAPE_INDEX_LONG_WALKS: u8 = 8;
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
    pub(super) fn root_shape_for_prototype(&mut self, prototype: Value) -> u32 {
        self.shapes.root_for_prototype(prototype)
    }

    pub(super) fn object_prototype(&self, object: Value) -> Value {
        self.object_data(object)
            .map(|object| self.prototype_for_data(object))
            .unwrap_or(Value::NULL)
    }

    pub(super) fn prototype_for_data(&self, object: &Object) -> Value {
        self.shapes[object.shape() as usize].prototype
    }

    pub(super) fn set_object_prototype(&mut self, value: Value, prototype: Value) {
        let current_shape = self
            .object_data(value)
            .expect("prototype owner is an object")
            .shape();
        if self.shapes[current_shape as usize].prototype == prototype {
            return;
        }
        let next_shape = self.rebase_shape_prototype(current_shape, prototype);
        self.object_data_mut(value)
            .expect("prototype owner survived shape rebase")
            .set_shape(next_shape);
    }

    pub(super) fn rebase_shape_prototype(&mut self, shape: u32, prototype: Value) -> u32 {
        let root = self.root_shape_for_prototype(prototype);
        let mut next = root;
        for (slot, entry) in super::gc::active_shape_attributes(&self.shapes, shape)
            .into_iter()
            .enumerate()
        {
            let slot = u32::try_from(slot).expect("object property index exceeds u32");
            let storage_len = slot as usize + 1;
            next = self.transition_shape_kind(
                next,
                match entry {
                    Some((key, _)) => ShapeTransition::Add { key, slot },
                    None => ShapeTransition::Vacant,
                },
                storage_len,
            );
            if let Some((_, attributes)) = entry
                && attributes != DEFAULT_PROPERTY_ATTRIBUTES
            {
                next = self.transition_shape_kind(
                    next,
                    ShapeTransition::Descriptor { slot, attributes },
                    storage_len,
                );
            }
        }
        if let Some(trigger) = self.shapes[shape as usize].dictionary_trigger {
            next = self.transition_shape_kind(
                next,
                ShapeTransition::Dictionary { trigger },
                self.shapes[shape as usize].storage_len,
            );
        }
        next
    }

    fn transition_shape_kind(
        &mut self,
        parent: u32,
        transition: ShapeTransition,
        storage_len: usize,
    ) -> u32 {
        let key = transition.cache_key();
        if let Some(key) = key
            && let Some(shape) = self.transitions.get(&(parent, key))
        {
            return *shape;
        }
        let shape = self.append_shape(parent, transition, storage_len);
        if let Some(key) = key {
            self.transitions.insert((parent, key), shape);
        }
        shape
    }

    /// Name-derived property classes, computed from the atom text once per atom.
    #[inline(always)]
    pub(super) fn atom_class(&self, atom: Atom) -> AtomClass {
        let known = AtomClass(self.atom_classes[atom as usize].get());
        if known.contains(AtomClass::DERIVED) {
            return known;
        }
        self.derive_atom_class(atom)
    }

    #[cold]
    #[inline(never)]
    fn derive_atom_class(&self, atom: Atom) -> AtomClass {
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
        if matches!(
            name,
            "length" | "size" | "byteLength" | "byteOffset" | "buffer"
        ) {
            bits |= AtomClass::VIRTUAL_FIELD;
        }
        if name.starts_with('\0') {
            bits |= AtomClass::INTERNAL;
        }
        if name.contains(crate::bytecode::SELF_BINDING_MARKER) {
            bits |= AtomClass::SELF_BINDING;
        }
        self.atom_classes[atom as usize].set(bits);
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
        let mut distance = 0;
        let slot = loop {
            let Some(id) = current else {
                break None;
            };
            let shape = &self.shapes[id as usize];
            match shape.transition {
                ShapeTransition::Add {
                    key: candidate,
                    slot,
                } if candidate == key => break Some(slot as usize),
                ShapeTransition::Delete { key: candidate, .. } if candidate == key => break None,
                ShapeTransition::Root
                | ShapeTransition::Add { .. }
                | ShapeTransition::Delete { .. }
                | ShapeTransition::Vacant
                | ShapeTransition::Descriptor { .. }
                | ShapeTransition::Dictionary { .. } => current = shape.parent,
            }
            distance += 1;
        };
        if distance >= SHAPE_INDEX_WALK_DISTANCE {
            self.note_long_shape_walk(shape);
        }
        slot
    }
    /// A shape whose lookups repeatedly walk long transition chains (such as
    /// the global object or a builtin prototype) gets its lookup index. The
    /// index is linear in the chain length and is built only after
    /// `SHAPE_INDEX_LONG_WALKS` walks of at least that order already ran, so
    /// index memory stays proportional to walking work already spent.
    #[cold]
    fn note_long_shape_walk(&self, shape: u32) {
        let entry = &self.shapes[shape as usize];
        let walks = entry.long_walks.get() + 1;
        if walks < SHAPE_INDEX_LONG_WALKS {
            entry.long_walks.set(walks);
            return;
        }
        entry
            .lookup_index
            .get_or_init(|| Box::new(derive_shape_lookup_index(&self.shapes, shape)));
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
        let parent_may_have_gc_roots = self.shapes[parent as usize].may_have_gc_roots;
        let prototype = self.shapes[parent as usize].prototype;
        self.shapes.push(Shape::child(
            Some(parent),
            transition,
            storage_len,
            dictionary_trigger,
            parent_may_have_gc_roots,
            prototype,
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
    /// The one place descriptors are recorded. Objects remember whether any index-keyed entry
    /// exists, so plain arrays can skip the per-index descriptor proof.
    pub(super) fn insert_descriptor(
        &mut self,
        object: Value,
        key: PropertyKey,
        attributes: PropertyAttributes,
    ) {
        if let PropertyKey::String(atom) = key
            && self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
            && let Some(data) = self.object_data_mut(object)
        {
            data.mark_indexed_descriptors();
        }
        self.descriptors.insert((object, key), attributes);
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
        self.insert_descriptor(object, key, attributes);
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
        self.field_add_indices.fill(NO_FIELD_ADD_SET);
        self.field_add_sets.clear();
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
    #[inline(always)]
    pub(super) fn checked_this_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
    ) -> Result<Value, JsError> {
        let activation = &self.frames[frame];
        if activation.fixed_this {
            return Ok(activation.this);
        }
        self.resolved_this_binding(p, frame)
    }

    /// `this` through the activation's lexical-this binding, for frames whose own `this` is
    /// not authoritative.
    #[inline(never)]
    fn resolved_this_binding(
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
        let shape = self.object_site_shape(program, site);
        self.heap.alloc_object_pair(shape, first, second)
    }
    #[inline(always)]
    pub(super) fn object_site_shape(&mut self, program: &ResidualProgram, site: usize) -> u32 {
        if self.specialized && program.specialized {
            let cache_site = self.object_cache_index(site);
            if self.object_shapes[cache_site] != u32::MAX {
                return self.object_shapes[cache_site];
            }
        }
        self.derive_object_site_shape(program, site)
    }

    /// Walks a literal site's transitions from the empty shape, recording the result when
    /// shape caches are active.
    #[inline(never)]
    fn derive_object_site_shape(&mut self, program: &ResidualProgram, site: usize) -> u32 {
        let root = self.root_shape_for_prototype(self.object_proto);
        let shape = program.object_sites[site]
            .atoms
            .iter()
            .fold(root, |shape, atom| self.transition_shape(shape, *atom));
        if self.specialized && program.specialized {
            let cache_site = self.object_cache_index(site);
            self.object_shapes[cache_site] = shape;
        }
        shape
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
    /// The target and handler of a Proxy, copied out so callers need not clone its cell.
    pub(super) fn proxy_parts(&self, value: Value) -> Option<(Value, Value)> {
        match self.heap.get(value)? {
            Cell::Proxy {
                target, handler, ..
            } => Some((*target, *handler)),
            _ => None,
        }
    }
    pub(super) fn object_data(&self, value: Value) -> Option<&Object> {
        self.heap.object(value)
    }
    pub(super) fn object_data_mut(&mut self, value: Value) -> Option<&mut Object> {
        self.heap.object_mut(value)
    }
    pub(super) fn is_object_like(&self, value: Value) -> bool {
        self.heap.object(value).is_some() || matches!(self.heap.get(value), Some(Cell::Error(_)))
    }
    #[inline(never)]
    pub(super) fn get_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Result<Value, JsError> {
        let heap_reference = object.heap_reference();
        if matches!(heap_reference, Some(HeapReference::ArrayRecord(_)))
            && self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
        {
            return self.get_property(p, object, atom);
        }
        let record_receiver = heap_reference.and_then(|reference| self.heap.record_object(reference));
        // A site is only populated after the atom and non-dictionary receiver and holder shapes
        // passed `shape_property_lookup_cell`, and an ordinary object is eligible whenever those
        // hold, so an own-data hit on ordinary objects needs no further eligibility probe. These
        // hits stay a leaf function; every other case continues out of line.
        if self.specialized
            && p.specialized
            && let Some(receiver) = record_receiver
        {
            if !receiver.is_module_namespace() {
                let index = self.field_cache_index(site);
                let shape = receiver.shape();
                // SAFETY: the active program's layout reserves every compiler-emitted site.
                let cache = unsafe { *self.field_caches.get_unchecked(index) };
                let slot = if cache.receiver == shape && cache.holder == NO_FIELD_HOLDER {
                    self.profile.field_cache_hit(0, 0);
                    Some(cache.slot)
                } else {
                    let slot = self.inline_megamorphic_field_slot(index, shape);
                    if slot.is_some() {
                        self.profile.field_cache_hit(2, 0);
                    }
                    slot
                };
                if let Some(slot) = slot {
                    // SAFETY: receiver shape and slot were recorded together for an own data
                    // property on the cache miss path.
                    return Ok(unsafe {
                        self.heap.property_get_unchecked(receiver, slot as usize)
                    });
                }
                return self.get_field_polymorphic(p, object, atom, site, heap_reference);
            }
        }
        self.get_field_uncached(p, object, atom, site, heap_reference)
    }

    /// An own-data slot from a site's inline megamorphic entries; the hashed overflow is
    /// left to `cached_field_value`.
    #[inline(always)]
    fn inline_megamorphic_field_slot(&self, site: usize, receiver: u32) -> Option<u16> {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors;
        // every non-sentinel index was installed together with its table.
        let table_index = unsafe { *self.megamorphic_field_indices.get_unchecked(site) };
        if table_index == NO_MEGAMORPHIC_FIELD {
            return None;
        }
        let set = unsafe { self.megamorphic_fields.get_unchecked(table_index as usize) };
        if set.overflow.is_some() {
            return None;
        }
        let slot = set.entries[..usize::from(set.len)]
            .iter()
            .find(|entry| entry.receiver == receiver && entry.holder == NO_FIELD_HOLDER)?
            .slot;
        Some(slot)
    }

    /// `get_field_cached` past the inline own-data hits: hashed megamorphic and
    /// prototype-holder hits, then the uncached path. The receiver is an ordinary object.
    #[inline(never)]
    fn get_field_polymorphic(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
        heap_reference: Option<HeapReference>,
    ) -> Result<Value, JsError> {
        let index = self.field_cache_index(site);
        if let Some(receiver) = heap_reference.and_then(|reference| self.heap.record_object(reference)) {
            if let Some(hit) = self
                .cached_field_value(index, receiver)
                .or_else(|| self.cached_holder_field_value(index, receiver, atom))
            {
                self.profile
                    .field_cache_hit(usize::from(hit.tier), hit.depth);
                return Ok(hit.value);
            }
            self.profile.field_cache(false);
            return self.get_field_miss(p, object, atom, index);
        }
        self.get_field_uncached(p, object, atom, site, heap_reference)
    }

    /// The receiver-shape miss path of `get_field_cached`: strings, proxies, prototype holders,
    /// cache population and generic lookup.
    #[inline(never)]
    fn get_field_uncached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
        heap_reference: Option<HeapReference>,
    ) -> Result<Value, JsError> {
        if let Some(receiver) = heap_reference.and_then(|reference| self.heap.record_object(reference)) {
            if self.specialized && p.specialized && !receiver.is_module_namespace() {
                let index = self.field_cache_index(site);
                if let Some(hit) = self
                    .cached_field_value(index, receiver)
                    .or_else(|| self.cached_holder_field_value(index, receiver, atom))
                {
                    self.profile
                        .field_cache_hit(usize::from(hit.tier), hit.depth);
                    return Ok(hit.value);
                }
            }
            self.profile.field_cache(false);
            return self.get_field_miss(p, object, atom, self.field_cache_index(site));
        }
        let legacy_cell = match heap_reference {
            Some(HeapReference::LegacySlot(index)) => self.heap.get_legacy(index),
            Some(HeapReference::ObjectRecord(_) | HeapReference::ArrayRecord(_)) | None => None,
        };
        let lookup = match legacy_cell {
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
        let holder_value = self.prototype_for_data(receiver);
        let holder = self.shape_property_lookup(holder_value, atom)?;
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
        let holder_value = self.object_prototype(receiver_value);
        let holder = self.shape_property_lookup(holder_value, atom)?;
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
        if let Some((target, handler)) = self.proxy_parts(target) {
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
            if let Some((target, handler)) = self.proxy_parts(current) {
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
            current = self.prototype_for_data(data);
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
        if !self.heap.array(receiver).is_some() {
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
        // Only an ordinary object reaches the shape cache, so a nullish base always falls
        // through to the uncached path's TypeError.
        if self.specialized
            && p.specialized
            && !self.is_private_name(atom)
            && let Some(data) = self.heap.object_record(object)
            && !data.is_module_namespace()
            && !data.is_arguments_object()
        {
            let site = self.field_cache_index(site);
            let (shape, proto, extensible) = (
                data.shape(),
                self.prototype_for_data(data),
                data.is_extensible(),
            );
            if self.try_cached_field_store(object, atom, shape, value, site)
                || (extensible
                    && self.field_cache_atom_eligible(atom)
                    && self.try_cached_field_add(object, atom, shape, proto, value, site))
            {
                return Ok(());
            }
        }
        self.set_field_uncached(p, object, atom, value, site, strict)
    }

    /// The shape-cache miss path of `set_field_cached`: nullish and primitive bases, proxies,
    /// private names and generic property assignment.
    #[inline(never)]
    fn set_field_uncached(
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
        if let Some((target, handler)) = self.proxy_parts(object) {
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
            && self.heap.array(object).is_some()
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
        let add_prototype_guard =
            if existing.is_none() && shape != u32::MAX && self.field_cache_atom_eligible(atom) {
                self.field_add_prototype_guard(object, atom)
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
                && let Some(prototype_guard) = add_prototype_guard
            {
                self.record_field_add_cache(
                    site,
                    FieldAddCache {
                        atom,
                        source_shape: shape,
                        target_shape: data_shape,
                        slot: slot as u16,
                        ..prototype_guard
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

    fn record_field_add_cache(&mut self, site: usize, cache: FieldAddCache) {
        let index = self.field_add_indices[site];
        if index == NO_FIELD_ADD_SET {
            self.field_add_indices[site] = self.field_add_sets.len() as u32;
            self.field_add_sets.push(FieldAddCacheSet::EMPTY);
        }
        let index = self.field_add_indices[site] as usize;
        self.field_add_sets[index].insert(cache);
    }

    #[inline(always)]
    fn try_cached_field_add(
        &mut self,
        object: Value,
        atom: Atom,
        shape: u32,
        proto: Value,
        value: Value,
        site: usize,
    ) -> bool {
        // The caller proved `object` an extensible ordinary object of `shape` with `proto`.
        // SAFETY: compiler-produced sites index exactly-sized cache vectors; every non-sentinel
        // index was installed together with its set.
        let set = unsafe { *self.field_add_indices.get_unchecked(site) };
        if set == NO_FIELD_ADD_SET {
            return false;
        }
        let Some(&cache) =
            unsafe { self.field_add_sets.get_unchecked(set as usize) }.find(shape, atom)
        else {
            return false;
        };
        // Entries record only non-dictionary source and prototype shapes, a shape's dictionary
        // state is fixed when it is created, and shape compaction clears these caches, so a
        // matching shape needs no dictionary probe here or along the prototype chain.
        if !self.field_add_prototype_chain_matches(proto, cache.prototype_shapes()) {
            return false;
        }
        debug_assert_eq!(cache.slot as usize, self.shapes[shape as usize].storage_len);
        self.heap
            .property_add(object, usize::from(cache.slot), value, cache.target_shape);
        if self.method_caches_may_name(atom) {
            self.invalidate_method_caches_for_prototype_add(object, atom);
        }
        self.profile.field_cache_hit(0, 0);
        true
    }

    /// The prototype shapes a cached add of `atom` to `object` must find again, as the depth and
    /// shape fields of a `FieldAddCache`.
    fn field_add_prototype_guard(&self, object: Value, atom: Atom) -> Option<FieldAddCache> {
        let mut prototype = self.object_prototype(object);
        let key = PropertyKey::string(atom);
        let mut guard = FieldAddCache::EMPTY;
        while !prototype.is_null() {
            let depth = usize::from(guard.prototype_depth);
            if depth == FIELD_ADD_CACHE_MAX_PROTO_DEPTH {
                return None;
            }
            let Some(data) = self.heap.object_record(prototype) else {
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
            guard.prototype_shapes[depth] = shape;
            guard.prototype_depth += 1;
            prototype = self.prototype_for_data(data);
        }
        Some(guard)
    }

    #[inline(always)]
    fn field_add_prototype_chain_matches(&self, mut prototype: Value, expected: &[u32]) -> bool {
        for expected_shape in expected {
            let Some(data) = self.heap.object_record(prototype) else {
                return false;
            };
            if data.shape() != *expected_shape
                || data.is_module_namespace()
                || data.is_arguments_object()
            {
                return false;
            }
            prototype = self.prototype_for_data(data);
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
        // Callable writes matter only to method caches, so atoms no cache names skip the probe.
        let invalidates_method = self.method_caches_may_name(atom)
            && self.cached_store_is_callable(object, cache.slot as usize, value);
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
        let mut current = self.object_data(object).map(|data| self.prototype_for_data(data));
        while let Some(value) = current.filter(|value| !value.is_null()) {
            if matches!(self.heap.get(value), Some(Cell::Proxy { .. })) {
                return true;
            }
            current = self.object_data(value).map(|data| self.prototype_for_data(data));
        }
        false
    }

    pub(super) fn prototype_chain_has_typed_array(&self, object: Value) -> bool {
        let mut current = self.object_data(object).map(|data| self.prototype_for_data(data));
        while let Some(value) = current.filter(|value| !value.is_null()) {
            if matches!(self.heap.get(value), Some(Cell::TypedArray { .. })) {
                return true;
            }
            current = self.object_data(value).map(|data| self.prototype_for_data(data));
        }
        false
    }

    pub(super) fn prototype_chain_has_indexed_set_exotic(&self, object: Value) -> bool {
        let mut current = self.object_data(object).map(|data| self.prototype_for_data(data));
        while let Some(value) = current.filter(|value| !value.is_null()) {
            if matches!(
                self.heap.get(value),
                Some(Cell::Proxy { .. } | Cell::TypedArray { .. })
            ) {
                return true;
            }
            current = self.object_data(value).map(|data| self.prototype_for_data(data));
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
            object = self.object_prototype(object);
            if object.is_null() {
                return None;
            }
        }
    }

    pub(super) fn inherited_write_blocked(&self, object: Value, atom: Atom) -> bool {
        let mut object = self.object_data(object).map(|data| self.prototype_for_data(data));
        while let Some(current) = object.filter(|value| !value.is_null()) {
            if let Some(attributes) = self.property_attributes(current, PropertyKey::string(atom)) {
                return !attributes.accessor && !attributes.writable;
            }
            object = self.object_data(current).map(|data| self.prototype_for_data(data));
        }
        false
    }
    /// `callable_write` for a slot a matching cached shape proved present: an ordinary
    /// object's old value is read in place rather than through `object_data`.
    #[inline(always)]
    fn cached_store_is_callable(&self, object: Value, slot: usize, value: Value) -> bool {
        if self.is_function(value) {
            return true;
        }
        let Some(data) = self.heap.object_record(object) else {
            return self.callable_write(object, slot, value);
        };
        // SAFETY: the matching cached shape proves the slot lies within the object's storage.
        let old = unsafe { self.heap.property_get_unchecked(data, slot) };
        self.is_function(old)
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
