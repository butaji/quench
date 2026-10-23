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
/// The shape of an object without properties, where every literal shape starts.
const EMPTY_SHAPE: u32 = 0;
/// Transitions a shape lookup may walk before the walk counts as long.
const SHAPE_INDEX_WALK_DISTANCE: usize = 32;
/// Long walks from one shape after which that shape gets a lookup index.
const SHAPE_INDEX_LONG_WALKS: u8 = 8;
pub(super) const FIELD_CACHE_SLOT_CAPACITY: usize = u16::MAX as usize + 1;
/// `AccessorFieldCache::holder` of an accessor the receiver holds itself.
const OWN_ACCESSOR: Value = Value::UNDEFINED;

struct FieldCacheHit {
    value: Value,
    tier: u8,
    depth: u16,
}

/// What a walk of an inherited entry's chain proved (`Vm::prove_inherited_chain`).
#[derive(Clone, Copy)]
struct InheritedChainProof {
    index: u32,
    prototype: Value,
    holder: Value,
    depth: u8,
    epoch: u64,
}

enum FieldCacheRead {
    Proxy,
    Generic,
    StringGeneric,
    Miss(usize),
    StringMiss { prototype: Value, site: usize },
    Hit(FieldCacheHit),
}

pub(super) fn derive_shape_lookup_index(shapes: &[Shape], shape: u32) -> ShapeLookupIndex {
    let mut entries = Vec::new();
    let mut seen_keys = FxHashSet::default();
    let mut seen_slots = FxHashSet::default();
    let mut attributes = FxHashMap::default();
    let mut current = Some(shape);
    while let Some(id) = current {
        let shape = &shapes[id as usize];
        match shape.transition {
            ShapeTransition::Add {
                key,
                slot,
                attributes: value,
            } => {
                if seen_keys.insert(key) {
                    entries.push((key, slot));
                }
                if seen_slots.insert(slot) && value != DEFAULT_DATA_ATTRIBUTES {
                    attributes.insert(slot, value.shape());
                }
            }
            ShapeTransition::Delete { key, slot } => {
                seen_keys.insert(key);
                seen_slots.insert(slot);
            }
            ShapeTransition::Descriptor {
                slot,
                attributes: value,
            } => {
                if seen_slots.insert(slot) && value != DEFAULT_SHAPE_ATTRIBUTES {
                    attributes.insert(slot, value);
                }
            }
            ShapeTransition::FunctionName { .. } | ShapeTransition::FunctionNameReconfigured | ShapeTransition::FunctionLengthReconfigured
            | ShapeTransition::Constructed { .. }
            | ShapeTransition::Root
            | ShapeTransition::Vacant
            | ShapeTransition::Dictionary { .. } => {}
        }
        current = shape.parent();
    }
    entries.reverse();
    entries.shrink_to_fit();
    ShapeLookupIndex::new(entries, attributes)
}

/// What the inline field caches know about one read.
pub(super) enum FieldCacheProbe {
    Hit(Value),
    /// An ordinary object the caches do not cover yet.
    OrdinaryMiss,
    /// A receiver or program outside the field caches.
    Ineligible,
}

impl<H: Host> Vm<H> {
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
            return index.slot(key).map(|slot| slot as usize);
        }
        if let Some(slot) = self.shape_slot_memo.get(shape, key) {
            return slot.map(|slot| slot as usize);
        }
        let slot = self.walked_property_shape_slot(shape, key);
        // A slot past u32 is never memoized; it is walked again.
        if let Some(recorded) = slot.map(u32::try_from).transpose().ok() {
            self.shape_slot_memo.record(shape, key, recorded);
        }
        slot
    }
    /// `property_shape_slot` of a non-dictionary shape, from its lookup index or its chain.
    fn walked_property_shape_slot(&self, shape: u32, key: PropertyKey) -> Option<usize> {
        if let Some(index) = self.shapes[shape as usize].lookup_index.get() {
            return index.slot(key).map(|slot| slot as usize);
        }
        // A key is only ever added to a shape that lacks it (`transition_data_property_shape`),
        // so a recorded add of `key` proves its absence without the walk. Objects that build
        // the same wide layout one key at a time (`__importStar`, decoded records) ask exactly
        // this on every intermediate shape; walking would also earn each of those shapes a
        // lookup index the size of its chain.
        if self.shapes[shape as usize].storage_len() >= SHAPE_INDEX_WALK_DISTANCE
            && self
                .transitions
                .get(&self.shapes, shape, ShapeTransitionKey::Add(key, DEFAULT_DATA_ATTRIBUTES))
                .is_some()
        {
            return None;
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
                    ..
                } if candidate == key => break Some(slot as usize),
                ShapeTransition::Delete { key: candidate, .. } if candidate == key => break None,
                ShapeTransition::FunctionName { .. } | ShapeTransition::FunctionNameReconfigured | ShapeTransition::FunctionLengthReconfigured
                | ShapeTransition::Constructed { .. }
                | ShapeTransition::Root
                | ShapeTransition::Add { .. }
                | ShapeTransition::Delete { .. }
                | ShapeTransition::Vacant
                | ShapeTransition::Descriptor { .. }
                | ShapeTransition::Dictionary { .. } => current = shape.parent(),
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
    /// The attributes of `object`'s shape-held property at `slot`, its getter read from the
    /// slot when the shape keeps it there (`SLOT_GETTER`).
    pub(super) fn slot_attributes(&self, object: &Object, slot: usize) -> Option<PropertyAttributes> {
        let attributes = self.shape_slot_attributes(object.shape(), slot)?;
        Some(attributes.attributes(&self.shape_accessors, || self.heap.property_get(object, slot)))
    }
    /// `object`'s data value at `slot`, as a raw read sees it: `undefined` for an accessor,
    /// whose slot holds its getter or nothing.
    pub(super) fn slot_data_value(&self, object: &Object, slot: usize) -> Option<Value> {
        let value = self.heap.property_get(object, slot)?;
        let shape = object.shape();
        if self.shapes[shape as usize].facts.slot_getters()
            && self
                .shape_slot_attributes(shape, slot)
                .is_some_and(ShapeAttributes::slot_getter)
        {
            return Some(Value::UNDEFINED);
        }
        Some(value)
    }
    fn shape_slot_attributes(&self, shape: u32, slot: usize) -> Option<ShapeAttributes> {
        if let Some(index) = self.shapes[shape as usize].lookup_index.get() {
            return Some(
                index
                    .attributes
                    .get(&(slot as u32))
                    .copied()
                    .unwrap_or(DEFAULT_SHAPE_ATTRIBUTES),
            );
        }
        let memo_slot = u32::try_from(slot).ok().filter(|_| !self.shape_is_dictionary(shape));
        if let Some(memo_slot) = memo_slot
            && let Some(attributes) = self.shape_attribute_memo.get(shape, memo_slot)
        {
            return attributes;
        }
        let attributes = self.walked_shape_attribute(shape, slot);
        if let Some(memo_slot) = memo_slot {
            self.shape_attribute_memo.record(shape, memo_slot, attributes);
        }
        attributes
    }
    /// The attributes the nearest transition describing `slot` gives it, walking `shape`'s chain.
    fn walked_shape_attribute(&self, shape: u32, slot: usize) -> Option<ShapeAttributes> {
        let mut current = Some(shape);
        while let Some(id) = current {
            let shape = &self.shapes[id as usize];
            match shape.transition {
                ShapeTransition::Descriptor {
                    slot: candidate,
                    attributes,
                } if candidate as usize == slot => {
                    return Some(attributes);
                }
                ShapeTransition::Add {
                    slot: candidate,
                    attributes,
                    ..
                } if candidate as usize == slot => {
                    return Some(attributes.shape());
                }
                ShapeTransition::Delete {
                    slot: candidate, ..
                } if candidate as usize == slot => return Some(DEFAULT_SHAPE_ATTRIBUTES),
                ShapeTransition::FunctionName { .. } | ShapeTransition::FunctionNameReconfigured | ShapeTransition::FunctionLengthReconfigured
                | ShapeTransition::Constructed { .. }
                | ShapeTransition::Root
                | ShapeTransition::Add { .. }
                | ShapeTransition::Delete { .. }
                | ShapeTransition::Vacant
                | ShapeTransition::Descriptor { .. }
                | ShapeTransition::Dictionary { .. } => current = shape.parent(),
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
        let parent_facts = self.shapes[parent as usize].facts;
        // The object that grew has moved to the child, so the parent's lookup index moves
        // with it, extended by the transition, and so do the long walks it has spent toward
        // one; an object left on the parent rebuilds it after `SHAPE_INDEX_LONG_WALKS` long
        // walks. Otherwise every intermediate shape of a growing wide object (the global
        // object during bootstrap) kept a full index, or each one walked its whole chain anew.
        let long_walks = self.shapes[parent as usize].long_walks.get();
        let lookup_index = self.shapes[parent as usize]
            .lookup_index
            .take()
            .and_then(|index| index.extended_by(transition));
        let mut child = Shape::child(
            Some(parent),
            transition,
            storage_len,
            dictionary_trigger,
            parent_facts,
        );
        child.long_walks.set(long_walks);
        if let Some(index) = lookup_index {
            child.lookup_index = OnceCell::from(index);
        }
        self.shapes.push(child);
        self.heap.register_property_shape(next, storage_len);
        next
    }
    /// Whether `to` is `from` extended by one appended property slot: the shape an add of
    /// one new data property makes, which `Heap::property_add` can apply directly.
    pub(super) fn shape_appends_one_slot(&self, from: u32, to: u32) -> bool {
        let (Some(from_shape), Some(to_shape)) = (self.shapes.get(from as usize), self.shapes.get(to as usize)) else {
            return false;
        };
        to_shape.parent == from
            && to_shape.storage_len == from_shape.storage_len + 1
            && !self.shape_is_dictionary(from)
            && !self.shape_is_dictionary(to)
    }

    pub(super) fn shape_is_dictionary(&self, shape: u32) -> bool {
        self.shapes[shape as usize].dictionary_trigger.is_some()
    }
    /// Whether a property of `shape` may be an accessor: a dictionary shape keeps its
    /// attributes outside its transitions, any other knows from its chain (`ShapeFacts`).
    #[inline(always)]
    pub(super) fn shape_may_have_accessors(&self, shape: u32) -> bool {
        let entry = &self.shapes[shape as usize];
        entry.dictionary_trigger.is_some() || entry.facts.accessors()
    }
    /// Whether a property of `shape` may be an accessor whose functions `shape_accessors`
    /// holds: a dictionary shape keeps its attributes outside its transitions, any other knows
    /// from its chain (`ShapeFacts`).
    #[inline(always)]
    pub(super) fn shape_may_have_table_accessors(&self, shape: u32) -> bool {
        let entry = &self.shapes[shape as usize];
        entry.dictionary_trigger.is_some() || entry.facts.table_accessors()
    }
    pub(super) fn mark_object_dictionary(&mut self, object: Value, trigger: DictionaryTrigger) {
        let Some(shape) = self.object_data(object).map(Object::shape) else {
            return;
        };
        if self.shape_is_dictionary(shape) {
            return;
        }
        let storage_len = self.shapes[shape as usize].storage_len();
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
        if let Some(data) = self.object_data(object)
            && let Some(slot) = self.property_shape_slot(data.shape(), key)
            && let Some(attributes) = self.slot_attributes(data, slot)
        {
            return Some(attributes);
        }
        // Non-shape properties, including array length and indexed elements,
        // keep explicit attributes in descriptors. Array length always exists;
        // its unchanged attributes derive from the array exotic contract.
        self.side_descriptor(object, key).or_else(|| {
            (key == PropertyKey::string(self.length_atom)
                && self.own_array_length(object).is_some())
            .then_some(super::object_array::ARRAY_LENGTH_ATTRIBUTES)
        })
    }
    /// `object`'s attributes for `key` in the descriptor table, which an object only has
    /// entries in once marked (`Object::has_side_descriptors`); any other value is looked up.
    #[inline(always)]
    pub(super) fn side_descriptor(&self, object: Value, key: PropertyKey) -> Option<PropertyAttributes> {
        if self.object_data(object).is_some_and(|data| !data.has_side_descriptors()) {
            return None;
        }
        self.descriptors.get(&(object, key)).copied()
    }
    /// The one place descriptors are recorded. Objects remember whether any index-keyed entry
    /// exists, so plain arrays can skip the per-index descriptor proof, and whether a `length`
    /// one does, so they can skip the lookup of their length's attributes.
    pub(super) fn insert_descriptor(
        &mut self,
        object: Value,
        key: PropertyKey,
        attributes: PropertyAttributes,
    ) {
        let (length, index) = match key {
            PropertyKey::String(atom) => {
                (atom == self.length_atom, self.atom_class(atom).contains(AtomClass::ARRAY_INDEX))
            }
            _ => (false, false),
        };
        if let Some(data) = self.object_data_mut(object) {
            data.mark_side_descriptors();
            if index {
                data.mark_indexed_descriptors();
            }
            if length {
                data.mark_length_descriptor();
            }
        }
        self.descriptors.insert((object, key), attributes);
    }

    /// The attributes of `holder`'s own property at array index `index`, named by `atom` when
    /// one exists (an index no atom names keys no named property): an element of an array or
    /// arguments object, or a named property such as a String wrapper's character. A frozen
    /// holder's elements are read-only.
    pub(super) fn own_index_attributes(
        &self,
        holder: Value,
        index: usize,
        atom: Option<Atom>,
    ) -> Option<PropertyAttributes> {
        if self.lacks_own_index(holder, index) {
            return None;
        }
        let named = atom.and_then(|atom| self.property_attributes(holder, PropertyKey::string(atom)));
        if self.own_element_present(holder, index) {
            let attributes = named.unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            let frozen = self.object_data(holder).is_some_and(Object::is_frozen);
            return Some(PropertyAttributes {
                writable: attributes.writable && !frozen,
                ..attributes
            });
        }
        let held = atom.is_some_and(|atom| self.own_property(holder, atom).is_some());
        named.filter(|attributes| held || attributes.accessor)
    }

    /// Whether `holder` provably has no own property at array index `index` without naming
    /// it: no element there, no index-keyed descriptor and no index name among its shape's
    /// keys.
    fn lacks_own_index(&self, holder: Value, index: usize) -> bool {
        self.object_data(holder).is_some_and(|data| {
            !data.has_indexed_descriptors() && !self.shape_has_index_key(data.shape())
        }) && !self.own_element_present(holder, index)
    }

    /// Whether any key of `shape` is an array-index name, memoized on the immutable shape
    /// from its nearest known ancestor. A dictionary shape answers conservatively.
    fn shape_has_index_key(&self, shape: u32) -> bool {
        if self.shape_is_dictionary(shape) {
            return true;
        }
        if let Some(known) = self.shapes[shape as usize].index_keys.get() {
            return known;
        }
        let mut current = Some(shape);
        let found = loop {
            let Some(id) = current else {
                break false;
            };
            let candidate = &self.shapes[id as usize];
            if let Some(known) = candidate.index_keys.get() {
                break known;
            }
            if let ShapeTransition::Add {
                key: PropertyKey::String(atom),
                ..
            } = candidate.transition
                && self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
            {
                break true;
            }
            current = candidate.parent();
        };
        self.shapes[shape as usize].index_keys.set(Some(found));
        found
    }

    /// Whether OrdinarySet of array index `index` on `object`, which holds no own property
    /// there, ends in a plain data definition on `object`: no prototype answers it with an
    /// accessor, read-only data or an exotic [[Set]] (proxy, typed array, module namespace).
    pub(super) fn inherited_index_unclaimed(&self, object: Value, index: usize) -> bool {
        let mut atom = None;
        let mut current = self.object_data(object).map(|data| data.proto());
        while let Some(holder) = current.filter(|value| !value.is_null()) {
            if matches!(
                self.heap.get(holder),
                Some(Cell::Proxy { .. } | Cell::TypedArray { .. })
            ) {
                return false;
            }
            let Some(data) = self.object_data(holder) else {
                return false;
            };
            if data.is_module_namespace() {
                return false;
            }
            if !self.lacks_own_index(holder, index) {
                let atom = *atom.get_or_insert_with(|| self.lookup_array_index_atom(index));
                if let Some(attributes) = self.own_index_attributes(holder, index, atom) {
                    return !attributes.accessor && attributes.writable;
                }
            }
            current = Some(data.proto());
        }
        true
    }

    /// Whether no prototype of `object` holds array index `index` or could answer it outside
    /// its storage (a proxy, typed array, module namespace, arguments object or boxed
    /// primitive): HasProperty of the index, where `object` holds none, is then false.
    pub(super) fn inherited_index_absent(&self, object: Value, index: usize) -> bool {
        let mut current = self.object_data(object).map(|data| data.proto());
        while let Some(holder) = current.filter(|value| !value.is_null()) {
            if matches!(self.heap.get(holder), Some(Cell::Proxy { .. } | Cell::TypedArray { .. })) {
                return false;
            }
            let Some(data) = self.object_data(holder) else {
                return false;
            };
            if data.is_module_namespace()
                || data.is_arguments_object()
                || data.boxed_primitive().is_some()
                || !self.lacks_own_index(holder, index)
            {
                return false;
            }
            current = Some(data.proto());
        }
        true
    }

    /// Whether storing each index of `indices`, none of them own, on `array` is a plain
    /// data definition (`inherited_index_unclaimed`).
    pub(super) fn array_indices_store_plainly(
        &self,
        array: Value,
        mut indices: std::ops::Range<usize>,
    ) -> bool {
        indices.all(|index| self.inherited_index_unclaimed(array, index))
    }

    pub(super) fn set_property_attributes(
        &mut self,
        object: Value,
        key: PropertyKey,
        attributes: PropertyAttributes,
    ) {
        if let Some((shape, slot)) = self.object_property_slot(object, key) {
            let current = self.slot_attributes(self.object_data(object).expect("shaped object"), slot);
            if current == Some(attributes) {
                return;
            }
            let recorded = ShapeAttributes::record(attributes, &mut self.shape_accessors);
            let next_id = self.append_cached_shape_transition(
                shape,
                ShapeTransition::Descriptor {
                    slot: slot as u32,
                    attributes: recorded,
                },
            );
            self.object_data_mut(object)
                .expect("object survived descriptor transition")
                .set_shape(next_id);
            self.store_accessor_slot(object, slot, current, attributes, recorded);
            self.invalidate_method_caches_for_key(key);
            return;
        }
        self.insert_descriptor(object, key, attributes);
        self.invalidate_field_caches();
        self.invalidate_method_caches_for_key(key);
    }
    /// `set_property_attributes` of each of `keys` (distinct) to `update` of its current
    /// attributes. A descriptor transition moves no slot and changes no other key, so every
    /// slot and current attribute set is read from the shape the object starts with, and the
    /// transitions then append without a lookup on each intermediate shape.
    pub(super) fn update_property_attributes(
        &mut self,
        object: Value,
        keys: &[PropertyKey],
        update: impl Fn(PropertyAttributes) -> PropertyAttributes,
    ) {
        let Some(original) = self.object_data(object).map(Object::shape) else {
            return;
        };
        let resolved = keys
            .iter()
            .map(|&key| {
                let slot = self.property_shape_slot(original, key);
                let current = slot
                    .and_then(|slot| self.slot_attributes(self.object_data(object)?, slot))
                    .or_else(|| self.property_attributes(object, key))
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                (key, slot, current)
            })
            .collect::<Vec<_>>();
        let mut shape = original;
        // Field caches read descriptors held outside shapes; one invalidation after the
        // loop covers every key it records, since nothing reads the caches meanwhile.
        let mut recorded_descriptors = false;
        for (key, slot, current) in resolved {
            let attributes = update(current);
            match slot {
                Some(_) if attributes == current => {}
                Some(slot) => {
                    let recorded = ShapeAttributes::record(attributes, &mut self.shape_accessors);
                    shape = self.append_cached_shape_transition(
                        shape,
                        ShapeTransition::Descriptor {
                            slot: slot as u32,
                            attributes: recorded,
                        },
                    );
                    self.store_accessor_slot(object, slot, Some(current), attributes, recorded);
                    self.invalidate_method_caches_for_key(key);
                }
                None => {
                    self.insert_descriptor(object, key, attributes);
                    recorded_descriptors = true;
                    self.invalidate_method_caches_for_key(key);
                }
            }
        }
        if recorded_descriptors {
            self.invalidate_field_caches();
        }
        if shape != original {
            self.object_data_mut(object)
                .expect("object survived descriptor transitions")
                .set_shape(shape);
        }
    }
    pub(super) fn remove_property_attributes(&mut self, object: Value, key: PropertyKey) {
        if let Some((shape, slot)) = self.object_property_slot(object, key) {
            let current = self.slot_attributes(self.object_data(object).expect("shaped object"), slot);
            self.store_accessor_slot(object, slot, current, DEFAULT_PROPERTY_ATTRIBUTES, DEFAULT_SHAPE_ATTRIBUTES);
            let next_id = self.append_cached_shape_transition(
                shape,
                ShapeTransition::Descriptor {
                    slot: slot as u32,
                    attributes: DEFAULT_SHAPE_ATTRIBUTES,
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
    /// Keeps the slot of `object`'s property at `slot` as its new `attributes` need it: an
    /// accessor's slot holds its `SLOT_GETTER` getter or `undefined`, and a data property made
    /// from an accessor starts as `undefined` (ValidateAndApplyPropertyDescriptor).
    fn store_accessor_slot(
        &mut self,
        object: Value,
        slot: usize,
        current: Option<PropertyAttributes>,
        attributes: PropertyAttributes,
        recorded: ShapeAttributes,
    ) {
        let value = if recorded.slot_getter() {
            attributes.getter.expect("a slot getter record has a getter")
        } else if attributes.accessor || current.is_some_and(|current| current.accessor) {
            Value::UNDEFINED
        } else {
            return;
        };
        self.heap.property_set(object, slot, value);
    }
    pub(super) fn invalidate_field_caches(&mut self) {
        match &mut self.field_cache_writes {
            Some(sites) => {
                for &site in sites.iter() {
                    let site = site as usize;
                    self.field_caches[site] = EMPTY_CACHE;
                    self.field_add_indices[site] = NO_FIELD_ADD_SET;
                    self.megamorphic_field_indices[site] = NO_MEGAMORPHIC_FIELD;
                    self.inherited_field_indices[site] = NO_INHERITED_FIELD;
                }
                sites.clear();
            }
            None => {
                self.field_caches.fill(EMPTY_CACHE);
                self.field_add_indices.fill(NO_FIELD_ADD_SET);
                self.megamorphic_field_indices.fill(NO_MEGAMORPHIC_FIELD);
                self.inherited_field_indices.fill(NO_INHERITED_FIELD);
                self.field_cache_writes = Some(Vec::new());
            }
        }
        self.symbol_write_proofs.clear();
        self.symbol_load_proofs.clear();
        self.array_iteration_proof = None;
        self.enumerable_keys.clear();
        self.field_add_sets.clear();
        self.megamorphic_fields.clear();
        self.inherited_field_caches.clear();
        self.accessor_field_caches.clear();
        self.instanceof_constructor_layouts = [None; INSTANCEOF_CONSTRUCTOR_LAYOUTS];
        self.closure_shapes = [None; 2];
        self.function_name_shapes.clear();
        self.closure_templates.fill(None);
        self.iterator_result_shape = None;
        self.reused_iterator_result = Value::UNDEFINED;
        self.descriptor_shapes = [None; 2];
        self.descriptor_layout = None;
        self.regexp_object_shapes = None;
        self.regexp_last_index_layouts.clear();
        self.regexp_builtin_layouts.clear();
        self.regexp_symbol_layouts.clear();
    }
    /// Notes that `site`'s per-site cache entries change (`field_cache_writes`).
    fn note_field_cache_write(&mut self, site: usize) {
        let limit = self.field_caches.len() / FIELD_CACHE_SPARSE_RESET_SHARE;
        if let Some(sites) = &mut self.field_cache_writes {
            if sites.len() < limit {
                sites.push(site as u32);
            } else {
                self.field_cache_writes = None;
            }
        }
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

    /// Resolve `this` through the activation's lexical-this binding.
    #[inline(never)]
    fn resolved_this_binding(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
    ) -> Result<Value, JsError> {
        self.lexical_this(frame).ok_or_else(|| {
            self.reference_error(
                p,
                "Must call super constructor before accessing 'this'".into(),
            )
        })
    }

    /// The frame's `this`, refreshed from its lexical binding; `None` while
    /// a derived constructor has not yet initialized it.
    #[inline(always)]
    pub(super) fn current_this(&mut self, frame: usize) -> Option<Value> {
        if self.frames[frame].fixed_this {
            return Some(self.frames[frame].this);
        }
        self.lexical_this(frame)
    }

    /// Refresh the frame's `this` from its lexical-this binding; `None` while a derived
    /// constructor has not yet initialized it.
    #[inline(never)]
    fn lexical_this(&mut self, frame: usize) -> Option<Value> {
        let atom = self.runtime_atoms.lexical_this;
        let value = self
            .dynamic_binding(frame, atom)
            .unwrap_or(self.frames[frame].this);
        // An initialized `this` binding never changes (a second `super()` throws), so the
        // frame keeps it as its fixed receiver; only a derived constructor's binding before
        // `super()` stays to be resolved again.
        let initialized = !value.is_deleted();
        let activation = &mut self.frames[frame];
        activation.this = value;
        activation.fixed_this = initialized;
        initialized.then_some(value)
    }

    #[inline(always)]
    pub(super) fn resolve_field_base(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        base: FieldBase,
    ) -> Result<Value, JsError> {
        if let Some(local) = base.local_index() {
            return self.load_local_binding(p, frame, usize::from(local), None);
        }
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
            holds_own: first.holder == NO_FIELD_HOLDER || second.holder == NO_FIELD_HOLDER,
            entries,
            overflow: None,
        }
    }
    /// The inline entries recorded so far. A walk of `len` entries without a slice bound,
    /// which would add a panic call to every lane read that inlines it.
    #[inline(always)]
    pub(super) fn recorded(&self) -> impl Iterator<Item = &FieldCache> {
        self.entries.iter().take(usize::from(self.len))
    }
    #[inline(always)]
    pub(super) fn get(&self, receiver: u32, holder: u32) -> Option<FieldCache> {
        let key = field_cache_key(receiver, holder);
        if let Some(entries) = &self.overflow {
            return entries.get(&key).copied();
        }
        self.recorded()
            .find(|entry| entry.receiver == receiver && entry.holder == holder)
            .copied()
    }
    fn insert(&mut self, cache: FieldCache) {
        self.holds_own |= cache.holder == NO_FIELD_HOLDER;
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
    /// A fresh array of the active realm holding `length` holes: `MakeArray`'s result.
    pub(super) fn hole_array(&mut self, length: usize) -> Value {
        self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                crate::heap::ArrayElements::filled(length, Value::DELETED),
            ),
        })
    }

    pub(super) fn object_pair(
        &mut self,
        program: &ResidualProgram,
        site: usize,
        first: Value,
        second: Value,
    ) -> Value {
        let shape = self.object_site_shape(program, site);
        self.heap
            .alloc_object_pair(self.object_proto, shape, first, second)
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

    /// A nested literal of constants built from its template (`LiteralStep`). No guest code
    /// and no collection runs while it is built, so the values on the build stack need no
    /// roots; each container is made as `MakeObjectLiteral` and `MakeConstArray` make one.
    pub(super) fn literal_template(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        template: usize,
    ) -> Result<Value, JsError> {
        let program_id = self.frames[frame].program;
        let steps = &program.literal_templates[template];
        let mut values: Vec<Value> = Vec::new();
        for step in steps.iter().copied() {
            let value = match step {
                crate::bytecode::LiteralStep::Constant(index) => {
                    self.programs.constant(program_id, index as usize).ok_or_else(|| {
                        JsError::validation("literal constant is outside program".into())
                    })?
                }
                crate::bytecode::LiteralStep::Array(count) => {
                    let elements = values.split_off(values.len() - count as usize);
                    self.heap.alloc(Cell::Array {
                        object: crate::heap::ArrayObject::new(
                            Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                            elements,
                        ),
                    })
                }
                crate::bytecode::LiteralStep::Object(site) => {
                    let shape = self.object_site_shape(program, site as usize);
                    let count = program.object_sites[site as usize].atoms.len();
                    let start = values.len() - count;
                    let object = self.heap.alloc_object_with_properties(
                        self.object_proto,
                        shape,
                        &values[start..],
                    );
                    values.truncate(start);
                    object
                }
            };
            values.push(value);
        }
        Ok(values.pop().expect("a validated template leaves its literal"))
    }

    /// Walks a literal site's transitions from the empty shape, recording the result when
    /// shape caches are active.
    #[inline(never)]
    fn derive_object_site_shape(&mut self, program: &ResidualProgram, site: usize) -> u32 {
        let shape = self.literal_shape(&program.object_sites[site].atoms);
        if self.specialized && program.specialized {
            let cache_site = self.object_cache_index(site);
            self.object_shapes[cache_site] = shape;
        }
        shape
    }
    /// The shape an empty object reaches by adding `atoms` as data properties in order: the
    /// transitions an object literal with those keys walks.
    pub(super) fn literal_shape(&mut self, atoms: &[Atom]) -> u32 {
        atoms
            .iter()
            .fold(EMPTY_SHAPE, |shape, atom| self.transition_shape(shape, *atom))
    }
    pub(super) fn own_property(&self, object: Value, atom: Atom) -> Option<Value> {
        if atom == self.length_atom
            && let Some(length) = self.own_array_length(object)
        {
            return Some(Value::length(length));
        }
        let object = self.object_data(object)?;
        let slot = self.shape_slot(object.shape(), atom)?;
        self.slot_data_value(object, slot)
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
    #[inline(always)]
    pub(super) fn get_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Result<Value, JsError> {
        self.prepare_named_function_info_lookup(object, atom);
        match self.cached_field_hit(p, object, site) {
            FieldCacheProbe::Hit(value) => Ok(value),
            FieldCacheProbe::OrdinaryMiss => match self.array_length_field(object, atom) {
                Some(length) => Ok(length),
                None => self.get_field_polymorphic(p, object, atom, site, object.heap_reference()),
            },
            FieldCacheProbe::Ineligible => match self.array_length_field(object, atom) {
                Some(length) => Ok(length),
                None => self.get_field_uncached(p, object, atom, site, object.heap_reference()),
            },
        }
    }

    /// `array.length`: an array's own `length` is a data property no definition can turn
    /// into an accessor or hide, so reading it needs no lookup.
    #[inline(always)]
    pub(super) fn array_length_field(&self, object: Value, atom: Atom) -> Option<Value> {
        (atom == self.length_atom)
            .then(|| self.own_array_length(object))
            .flatten()
            .map(Value::length)
    }

    /// Probe own and holder entries using the shape pinned by the receiver.
    #[inline(always)]
    pub(super) fn cached_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> FieldCacheProbe {
        if !(self.specialized && p.specialized) {
            return FieldCacheProbe::Ineligible;
        }
        let Some(receiver) = self.heap.object(object) else {
            return FieldCacheProbe::Ineligible;
        };
        let index = self.field_cache_index(site);
        if let Some(hit) = self.cached_field_value(index, receiver) {
            self.profile
                .field_cache_hit(usize::from(hit.tier), hit.depth);
            return FieldCacheProbe::Hit(hit.value);
        }
        if receiver.is_module_namespace() {
            return FieldCacheProbe::Ineligible;
        }
        if let Some(hit) = self.cached_ordinary_holder_field_value(index, receiver) {
            self.profile
                .field_cache_hit(usize::from(hit.tier), hit.depth);
            return FieldCacheProbe::Hit(hit.value);
        }
        FieldCacheProbe::OrdinaryMiss
    }

    /// The own data field of a direct object that the site's primary entry names: the read
    /// `cached_field_hit` answers first, without its other tiers.
    #[inline(always)]
    pub(super) fn primary_own_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.lane_admits(p));
        let receiver = self.heap.object_record_by_id(object.direct_object_index()?);
        let cache = self.field_cache_entry(site);
        if cache.holder != NO_FIELD_HOLDER {
            return None;
        }
        let value = self.own_field_entry_hit(cache, receiver)?;
        self.profile.field_cache_hit(0, 0);
        Some(value)
    }

    /// The own data field the holderless field cache entry `cache` proves for `receiver`;
    /// `None` where its shape differs.
    #[inline(always)]
    fn own_field_entry_hit(&self, cache: FieldCache, receiver: &Object) -> Option<Value> {
        // The exact shape matches only outside dictionary storage (`Object::exact_shape`).
        if cache.receiver != receiver.exact_shape() {
            return None;
        }
        // SAFETY: as for `cached_field_hit`; the exact match rules out a dictionary.
        Some(unsafe { self.heap.property_get_flat(receiver, cache.slot as usize) })
    }

    /// The field cache entry of the active program's `site`.
    #[inline(always)]
    pub(super) fn field_cache_entry(&self, site: u16) -> FieldCache {
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        unsafe { *self.field_caches.get_unchecked(self.field_cache_index(site)) }
    }

    /// The value the field cache entry `cache` proves for the direct object `object`: its own
    /// data field where the entry names no holder, else that of its prototype, an object
    /// record; `None` where either shape differs. The primary own and record-holder reads
    /// make it of their site's entry; a call's read memo makes it of a copy, which holds
    /// while the shapes it names do.
    #[inline(always)]
    pub(super) fn record_field_entry_hit(&self, cache: FieldCache, object: Value) -> Option<Value> {
        let receiver = self.heap.object_record_by_id(object.direct_object_index()?);
        if cache.holder == NO_FIELD_HOLDER {
            return self.own_field_entry_hit(cache, receiver);
        }
        let holder = self.heap.object_record_by_id(receiver.proto_link().object_record_id()?);
        if cache.receiver != receiver.exact_shape()
            || holder.is_module_namespace()
            || cache.holder != holder.exact_shape()
        {
            return None;
        }
        // SAFETY: as `cached_ordinary_holder_field_value`.
        Some(unsafe { self.heap.property_get_flat(holder, cache.slot as usize) })
    }

    /// The store `cached_field_store_hit` makes to an own data field of a direct object that
    /// the site's primary entry names, without its other tiers. Without `LOGS`, `false` also
    /// leaves a store the write barrier logs (`Heap::record_property_set_flat`).
    #[inline(always)]
    pub(super) fn primary_own_field_store_hit<const LOGS: bool>(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> bool {
        debug_assert!(self.lane_admits(p));
        if self.method_caches_may_name(atom) {
            return false;
        }
        let Some(record_id) = object.direct_object_index() else {
            return false;
        };
        let receiver = self.heap.object_record_by_id(record_id);
        if receiver.is_module_namespace() || receiver.is_arguments_object() {
            return false;
        }
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = unsafe { *self.field_caches.get_unchecked(self.field_cache_index(site)) };
        if cache.receiver != receiver.exact_shape() || cache.holder != NO_FIELD_HOLDER {
            return false;
        }
        // SAFETY: a matching exact shape proves the cached slot layout outside dictionary
        // storage (`Object::exact_shape`).
        if !unsafe { self.heap.record_property_set_flat::<LOGS>(record_id, cache.slot as usize, value) } {
            return false;
        }
        self.profile.field_cache_hit(0, 0);
        true
    }

    /// The add `cached_field_add_hit` makes for a lane: an ordinary record's cached add,
    /// without the specialization test (`Vm::lane_admits`) or the store probe before it. The
    /// caller proved `atom` eligible (`field_add_atom_eligible`) for the site.
    #[inline(always)]
    pub(super) fn primary_field_add_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> bool {
        debug_assert!(self.lane_admits(p) && self.field_add_atom_eligible(atom));
        let Some(data) = self.heap.object_record(object) else {
            return false;
        };
        if !data.admits_cached_add() {
            return false;
        }
        let (shape, link) = (data.shape(), data.proto_link());
        let site = self.field_cache_index(site);
        self.try_cached_field_add(object, atom, shape, link, value, site)
    }

    /// `primary_field_add_hit` while the site's entry for the receiver's shape holds its
    /// chain proof (`prove_field_add_chain`) and no method cache names the key, for a receiver
    /// that is an object record: no walk, and a plain append (`Object::admits_plain_append`)
    /// the write barrier does not log makes no call, so a lane handler inlining it keeps its
    /// common path short; an add whose storage moves is made out of line
    /// (`Heap::record_property_add_moving`) and may allocate. `false` changes nothing and
    /// leaves the add to `primary_field_add_hit`.
    #[inline(always)]
    pub(super) fn proved_field_add_hit(&mut self, object: Value, atom: Atom, value: Value, site: u16) -> bool {
        let Some(record_id) = object.direct_object_index() else {
            return false;
        };
        let receiver = self.heap.object_record_by_id(record_id);
        // The exact shape matches an entry's source, which is never a dictionary's, only
        // outside dictionary storage (`Object::exact_shape`).
        let Some((_, cache)) = self.field_add_entry(self.field_cache_index(site), receiver.exact_shape(), atom) else {
            return false;
        };
        let (target_shape, slot) = (cache.target_shape, usize::from(cache.slot));
        if !cache.proof_holds(receiver.proto_link())
            || !receiver.admits_cached_add()
            || self.method_caches_may_name(atom)
        {
            return false;
        }
        if !(receiver.admits_plain_append(slot, value)
            && self.heap.record_property_append_plain(record_id, slot, value, target_shape))
        {
            self.heap.record_property_add_moving(object, slot, value, target_shape);
        }
        self.profile.field_cache_hit(0, 0);
        true
    }

    /// Whether the site has recorded a cached property add (`try_cached_field_add`).
    pub(super) fn field_site_adds(&self, site: u16) -> bool {
        self.field_add_indices[self.field_cache_index(site)] != NO_FIELD_ADD_SET
    }

    /// The data field a direct object's immediate prototype holds at the site's primary
    /// holder entry: `cached_field_hit`'s holder tier for a site whose primary entry names a
    /// prototype (`field_site_holder`).
    #[inline(always)]
    pub(super) fn primary_holder_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.lane_admits(p));
        let receiver = self.heap.object_record_by_id(object.direct_object_index()?);
        let hit = self.cached_ordinary_holder_field_value(self.field_cache_index(site), receiver)?;
        self.profile
            .field_cache_hit(usize::from(hit.tier), hit.depth);
        Some(hit.value)
    }

    /// `primary_holder_field_hit` when the receiver's prototype is an object record, the
    /// common holder, read without the cell families' dispatch (`Heap::object`), which is a
    /// call: `None` for any other prototype as for a miss.
    #[inline(always)]
    pub(super) fn primary_record_holder_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.lane_admits(p));
        let cache = self.field_cache_entry(site);
        if cache.holder == NO_FIELD_HOLDER {
            return None;
        }
        let value = self.record_field_entry_hit(cache, object)?;
        self.profile.field_cache_hit(0, 1);
        Some(value)
    }

    /// `primary_own_field_hit` for a receiver any heap cell holds, such as a function's own
    /// static field (`Strength.REQUIRED`), which a direct object's record lookup misses.
    #[inline(always)]
    pub(super) fn cell_own_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.lane_admits(p));
        let receiver = self.heap.object(object)?;
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = unsafe { *self.field_caches.get_unchecked(self.field_cache_index(site)) };
        if cache.receiver != receiver.exact_shape() || cache.holder != NO_FIELD_HOLDER {
            return None;
        }
        // SAFETY: as for `cached_field_hit`; the exact match rules out a dictionary.
        let value = unsafe { self.heap.property_get_flat(receiver, cache.slot as usize) };
        self.profile.field_cache_hit(0, 0);
        Some(value)
    }

    /// `primary_holder_field_hit` for a receiver any heap cell holds, such as a function whose
    /// prototype holds the method (`f.apply`), which a direct object's record lookup misses.
    #[inline(always)]
    pub(super) fn cell_holder_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.lane_admits(p));
        let receiver = self.heap.object(object)?;
        let hit = self.cached_ordinary_holder_field_value(self.field_cache_index(site), receiver)?;
        self.profile
            .field_cache_hit(usize::from(hit.tier), hit.depth);
        Some(hit.value)
    }

    /// The data field a direct object inherits from beyond its immediate prototype, at the
    /// site's inherited entry (`cached_inherited_field_value`): `field_miss_data_value`'s
    /// cache hit for a site that has one (`field_site_inherited`).
    #[inline(always)]
    pub(super) fn inherited_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.lane_admits(p));
        let (receiver, function_receiver) = self.inherited_receiver(object)?;
        let (hit, proof) =
            self.cached_inherited_field_value(self.field_cache_index(site), receiver, function_receiver)?;
        if let Some(proof) = proof {
            self.prove_inherited_chain(proof);
        }
        self.profile
            .field_cache_hit(usize::from(hit.tier), hit.depth);
        Some(hit.value)
    }

    /// `inherited_field_hit` while the entry's proof holds (the prototype layout epoch and
    /// the receiver's prototype it was made under) and the holder is an object record: no
    /// chain walk and no call, so a lane handler inlining it saves no registers. `None`
    /// leaves any other read to `inherited_field_hit`.
    #[inline(always)]
    pub(super) fn proved_inherited_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.lane_admits(p));
        let (receiver, function_receiver) = self.inherited_receiver(object)?;
        // SAFETY: as for `cached_inherited_field_value`.
        let index = unsafe { *self.inherited_field_indices.get_unchecked(self.field_cache_index(site)) };
        if index == NO_INHERITED_FIELD {
            return None;
        }
        let cache = unsafe { self.inherited_field_caches.get_unchecked(index as usize) };
        if cache.receiver != receiver.shape()
            || function_receiver && !cache.function_links
            || cache.proved_epoch != crate::heap::prototype_layout_epoch()
            || cache.proved_prototype != receiver.proto_link()
        {
            return None;
        }
        let value = match cache.answer {
            InheritedFieldAnswer::Slot(slot) => {
                let holder = self.heap.object_record(cache.proved_holder)?;
                // SAFETY: the proof keeps the holder and its recorded shape.
                unsafe { self.heap.property_get_shaped(holder, usize::from(slot)) }?
            }
            InheritedFieldAnswer::Absent => Value::UNDEFINED,
        };
        let depth = u16::from(cache.depth);
        self.profile.field_cache_hit(0, depth);
        Some(value)
    }

    /// The object an inherited entry's receiver test reads, and whether it is a function's:
    /// an object record, or a function, which an entry answers only where its shape holds
    /// the name (`InheritedFieldCache::function_links`).
    #[inline(always)]
    fn inherited_receiver(&self, value: Value) -> Option<(&Object, bool)> {
        match value.direct_object_index() {
            Some(record_id) => Some((self.heap.object_record_by_id(record_id), false)),
            None => self.heap.function(value).map(|function| (&function.object, true)),
        }
    }

    /// Whether the site has an inherited entry (`inherited_field_hit`).
    pub(super) fn field_site_inherited(&self, site: u16) -> bool {
        self.inherited_field_indices[self.field_cache_index(site)] != NO_INHERITED_FIELD
    }

    /// Whether a read at the site met an accessor it cached (`cached_field_getter`).
    pub(super) fn field_site_getter(&self, site: u16) -> bool {
        self.accessor_field_caches.contains_key(&self.field_cache_index(site))
    }

    /// Whether the site's primary entry names a prototype holder (`primary_holder_field_hit`).
    pub(super) fn field_site_holder(&self, site: u16) -> bool {
        self.field_caches[self.field_cache_index(site)].holder != NO_FIELD_HOLDER
    }

    /// The data field of a direct object, or of its immediate prototype held in an object
    /// record, among the site's megamorphic entries, which include the primary entry:
    /// `cached_field_hit` and its holder tier for a site past one shape.
    #[inline(always)]
    pub(super) fn polymorphic_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        self.polymorphic_field_entries_hit::<true>(p, object, site)
    }

    /// `polymorphic_field_hit` among the inline entries alone: `None` for a site whose
    /// entries moved to the hash, which the general cached read (`cached_field_polymorphic_hit`)
    /// answers. Its probe keeps few values live, so a lane handler inlining it saves no
    /// registers.
    #[inline(always)]
    pub(super) fn inline_polymorphic_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        self.polymorphic_field_entries_hit::<false>(p, object, site)
    }

    /// `polymorphic_field_hit`, probing the hashed entries where `HASHED`.
    #[inline(always)]
    fn polymorphic_field_entries_hit<const HASHED: bool>(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.lane_admits(p));
        // Any record receiver, an array's too: entries name only shape slots, which an
        // array's length and elements are not.
        let receiver = self.heap.record_object_value(object)?;
        let index = self.field_cache_index(site);
        // SAFETY: compiler-produced sites index exactly-sized cache vectors;
        // every non-sentinel index was installed together with its table.
        let table_index = unsafe { *self.megamorphic_field_indices.get_unchecked(index) };
        if table_index == NO_MEGAMORPHIC_FIELD {
            return None;
        }
        let set = unsafe { self.megamorphic_fields.get_unchecked(table_index as usize) };
        // Exact shapes: a match proves a layout outside dictionary storage, so each hit
        // reads its slot flat (`Object::exact_shape`).
        let shape = receiver.exact_shape();
        if set.overflow.is_some() {
            if !HASHED {
                return None;
            }
            // The hashed entries: an own one for the receiver's shape, else one for its
            // immediate prototype in an object record. A shape either holds the atom or not,
            // so at most one of the two is recorded; a set of holders alone has no own one.
            if set.holds_own
                && let Some(entry) = set.get(shape, NO_FIELD_HOLDER)
            {
                // SAFETY: as for `cached_field_hit`.
                let value = unsafe { self.heap.property_get_flat(receiver, usize::from(entry.slot)) };
                self.profile.field_cache_hit(2, 0);
                return Some(value);
            }
            let holder = self.heap.record_prototype(receiver)?;
            if holder.is_module_namespace() {
                return None;
            }
            let entry = set.get(shape, holder.exact_shape())?;
            // SAFETY: as for the inline holder entries below.
            let value = unsafe { self.heap.property_get_flat(holder, usize::from(entry.slot)) };
            self.profile.field_cache_hit(2, 1);
            return Some(value);
        }
        for entry in set.recorded() {
            if entry.receiver != shape {
                continue;
            }
            if entry.holder == NO_FIELD_HOLDER {
                // SAFETY: as for `cached_field_hit`.
                let value = unsafe { self.heap.property_get_flat(receiver, usize::from(entry.slot)) };
                self.profile.field_cache_hit(2, 0);
                return Some(value);
            }
            // A prototype outside the records takes the slow path.
            let holder = self.heap.record_prototype(receiver)?;
            if holder.is_module_namespace() || holder.exact_shape() != entry.holder {
                continue;
            }
            // SAFETY: both immutable shapes and the property slot were recorded together
            // after confirming an immediate-prototype data property.
            let value = unsafe { self.heap.property_get_flat(holder, usize::from(entry.slot)) };
            self.profile.field_cache_hit(2, 1);
            return Some(value);
        }
        None
    }

    /// `get_field_polymorphic` for an object receiver up to the point where it
    /// would run guest code: hashed megamorphic and prototype-holder entries, then the data
    /// lookup of `get_field_miss`. `None` leaves the read to `get_property`.
    pub(super) fn cached_field_polymorphic_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Option<Value> {
        if !(self.specialized && p.specialized) {
            return None;
        }
        let index = self.field_cache_index(site);
        // A record's own and holder entries; any other object (a function, say) goes
        // straight to the data lookup, whose shape proof covers every object kind.
        let receiver = self.heap.record_object(object.heap_reference()?);
        if let Some(hit) = receiver.and_then(|receiver| {
            self.cached_field_value(index, receiver)
                .or_else(|| self.cached_holder_field_value(index, receiver, atom))
        }) {
            self.profile
                .field_cache_hit(usize::from(hit.tier), hit.depth);
            return Some(hit.value);
        }
        self.profile.field_cache(false);
        self.field_miss_data_value(object, atom, index)
    }

    /// Whether the site's shapes moved to megamorphic entries, inline or hashed:
    /// `polymorphic_field_hit` then sees every entry the site has recorded.
    pub(super) fn field_site_polymorphic(&self, site: u16) -> bool {
        self.megamorphic_field_indices[self.field_cache_index(site)] != NO_MEGAMORPHIC_FIELD
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
        let slot = set
            .recorded()
            .find(|entry| entry.receiver == receiver && entry.holder == NO_FIELD_HOLDER)?
            .slot;
        Some(slot)
    }

    /// An own-data slot from a site's hashed megamorphic entries, once its inline entries
    /// overflowed: the store at an AST-building site sees a receiver shape per node kind
    /// (Babel's and TypeScript's `node.end =` and `e.parent =`), so its writes stay cached
    /// in the lane instead of each leaving it. Out of line, as the hash probe needs
    /// registers the inline store does not.
    #[inline(never)]
    fn hashed_megamorphic_own_field_slot(&self, site: usize, receiver: u32) -> Option<u16> {
        // SAFETY: as for `inline_megamorphic_field_slot`.
        let table_index = unsafe { *self.megamorphic_field_indices.get_unchecked(site) };
        if table_index == NO_MEGAMORPHIC_FIELD {
            return None;
        }
        let set = unsafe { self.megamorphic_fields.get_unchecked(table_index as usize) };
        set.overflow.as_ref()?;
        set.get(receiver, NO_FIELD_HOLDER).map(|entry| entry.slot)
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
        if let Some(receiver) =
            heap_reference.and_then(|reference| self.heap.record_object(reference))
        {
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
        if let Some(receiver) =
            heap_reference.and_then(|reference| self.heap.record_object(reference))
        {
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
            // Function and iterator records have no cell; their shape answers like the
            // slot forms' (`shape_property_lookup`).
            Some(HeapReference::FunctionRecord(_) | HeapReference::IteratorRecord(_)) => {
                let lookup = if self.specialized && p.specialized {
                    self.receiver_field_cache_read(self.shape_property_lookup(object, atom), atom, site)
                } else {
                    FieldCacheRead::Generic
                };
                return self.finish_field_cache_read(p, object, atom, lookup);
            }
            Some(HeapReference::StringRecord(_)) => {
                let lookup = if self.specialized && p.specialized {
                    match self.string_field_cache_read(atom, site) {
                        Some(Some(hit)) => FieldCacheRead::Hit(hit),
                        Some(None) => FieldCacheRead::StringMiss {
                            prototype: self.string_proto,
                            site: self.field_cache_index(site),
                        },
                        None => FieldCacheRead::StringGeneric,
                    }
                } else {
                    FieldCacheRead::Generic
                };
                return self.finish_field_cache_read(p, object, atom, lookup);
            }
            Some(
                HeapReference::ObjectRecord(_)
                | HeapReference::ArrayRecord(_)
                | HeapReference::PromiseRecord(_)
                | HeapReference::AwaitRecord(_)
                | HeapReference::V128Record(_),
            )
            | None => None,
        };
        if matches!(legacy_cell, Some(Cell::TypedArray { .. }))
            && self.atom_class(atom).contains(AtomClass::VIRTUAL_FIELD)
        {
            if let Some(value) = self.typed_array_geometry_read(object, atom) {
                return Ok(value);
            }
            return self.finish_field_cache_read(p, object, atom, FieldCacheRead::Generic);
        }
        let lookup = match legacy_cell {
            Some(Cell::Proxy { .. }) => FieldCacheRead::Proxy,
            Some(cell) if self.specialized && p.specialized => {
                self.object_field_cache_read(cell, atom, site)
            }
            Some(_) | None => FieldCacheRead::Generic,
        };
        self.finish_field_cache_read(p, object, atom, lookup)
    }

    /// Completes a field read from the field cache's answer for its receiver.
    fn finish_field_cache_read(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        lookup: FieldCacheRead,
    ) -> Result<Value, JsError> {
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

    /// The field cache's answer for a string primitive's read of `atom`, which
    /// `String.prototype` answers: `None` when its shape cannot answer it, else the site's
    /// hit, if any.
    #[inline(always)]
    fn string_field_cache_read(&self, atom: Atom, site: u16) -> Option<Option<FieldCacheHit>> {
        if !self.field_cache_atom_eligible(atom) {
            return None;
        }
        let receiver = self.shape_property_lookup(self.string_proto, atom)?;
        Some(self.cached_field_value(self.field_cache_index(site), receiver))
    }

    /// A string primitive's `GetField` where no lookup can run guest code, in the order
    /// `get_field_cached` takes: its `length`, then the `String.prototype` data property
    /// its site cache holds (`string_prototype_field_hit`). An index, or any other key, is
    /// left to the general path.
    pub(super) fn cached_string_field_hit(
        &self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Option<Value> {
        let text = self.heap.string(object)?;
        if atom == self.length_atom {
            return Some(Value::number(text.len() as f64));
        }
        if !self.string_field_site_cacheable(p, atom) {
            return None;
        }
        // A name the cache does not hold, as an absent `type`, is a data walk of the
        // prototype chain, which the string's own index and `length` keys never shadow.
        self.string_prototype_field_hit(site)
            .or_else(|| self.plain_named_get(self.string_proto, atom))
    }

    /// Whether a string primitive's read of `atom` in `p` can take the `String.prototype`
    /// data property its site caches (`string_prototype_field_hit`): specialized code
    /// reading neither `length` nor an index, which the string itself answers, nor a name
    /// no shape answers. It depends on the site alone, so a handler checks it once.
    pub(super) fn string_field_site_cacheable(&self, p: &ResidualProgram, atom: Atom) -> bool {
        atom != self.length_atom
            && self.specialized
            && p.specialized
            && !self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
            && self.field_cache_atom_eligible(atom)
    }

    /// The `String.prototype` data property site `site` caches, at a site that
    /// `string_field_site_cacheable` admits.
    #[inline(always)]
    pub(super) fn string_prototype_field_hit(&self, site: u16) -> Option<Value> {
        let prototype = self.heap.object_record(self.string_proto)?;
        if self.shape_is_dictionary(prototype.shape()) {
            return None;
        }
        Some(self.cached_field_value(self.field_cache_index(site), prototype)?.value)
    }

    /// The field cache's answer for an object cell other than a string or a
    /// proxy: a hit, a miss at the resolved site, or no cacheable receiver.
    #[inline(always)]
    fn object_field_cache_read(&self, cell: &Cell, atom: Atom, site: u16) -> FieldCacheRead {
        self.receiver_field_cache_read(self.shape_property_lookup_cell(cell, atom), atom, site)
    }

    /// The field cache's answer for a receiver whose shape answers the read (`None`: it
    /// does not).
    #[inline(always)]
    fn receiver_field_cache_read(
        &self,
        receiver: Option<&Object>,
        atom: Atom,
        site: u16,
    ) -> FieldCacheRead {
        let Some(receiver) = receiver else {
            return FieldCacheRead::Generic;
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

    /// The monomorphic prototype-holder hit when the immediate prototype is an ordinary object;
    /// any other holder takes `cached_holder_field_value` on the uncached path.
    #[inline(always)]
    fn cached_ordinary_holder_field_value(
        &self,
        site: usize,
        receiver: &Object,
    ) -> Option<FieldCacheHit> {
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = unsafe { *self.field_caches.get_unchecked(site) };
        if cache.holder == NO_FIELD_HOLDER || cache.receiver != receiver.exact_shape() {
            return None;
        }
        // Most prototypes are object records, read from the stored link without decoding it
        // or the cell families' dispatch.
        let holder = match receiver.proto_link().object_record_id() {
            Some(record) => self.heap.object_record_by_id(record),
            None => self.heap.object(receiver.proto())?,
        };
        if holder.is_module_namespace() || cache.holder != holder.exact_shape() {
            return None;
        }
        // SAFETY: both immutable shapes and the property slot were recorded together after
        // confirming an immediate-prototype data property; the exact holder match rules out
        // a dictionary.
        let value = unsafe { self.heap.property_get_flat(holder, cache.slot as usize) };
        Some(FieldCacheHit {
            value,
            tier: 0,
            depth: 1,
        })
    }

    #[inline(always)]
    fn cached_holder_field_value(
        &self,
        site: usize,
        receiver: &Object,
        atom: Atom,
    ) -> Option<FieldCacheHit> {
        // A record prototype (an object's, or an array's as `Array.prototype`) is the holder as
        // it stands: entries record only holder shapes whose lookup answered the atom
        // (`shape_property_lookup`), and a shape fixes its dictionary state and keys, so a
        // matching entry proves what the lookup would. An array's length and elements are no
        // shape slots, so no entry names them.
        let holder = match self.heap.record_prototype(receiver) {
            Some(holder) if !holder.is_module_namespace() => holder,
            _ => self.shape_property_lookup(receiver.proto(), atom)?,
        };
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
        match self.field_miss_data_value(object, atom, site) {
            Some(value) => Ok(value),
            None => {
                // A getter the site's accessor entry answers is the `[[Get]]` result's whole
                // computation: its call with the receiver, without walking the chain again.
                let getter = self.cached_field_getter(object, atom, site).or_else(|| {
                    self.record_accessor_field_cache(object, atom, site);
                    self.cached_field_getter(object, atom, site)
                });
                #[cfg(feature = "profile-lane-exits")]
                if getter.is_none() {
                    super::lane_exits::record_detail(format!(
                        "field miss {} function={} shape_answers={} dictionary={}",
                        self.atom_name(atom),
                        self.heap.function(object).is_some(),
                        self.shape_property_lookup(object, atom).is_some(),
                        self.object_data(object).is_some_and(|data| self.shape_is_dictionary(data.shape())),
                    ));
                }
                match getter {
                    Some(getter) => self.call_value(p, getter, object, &[]),
                    None => self.get_property(p, object, atom),
                }
            }
        }
    }

    /// Records the accessor a field read at `site` meets on `object` or a prototype of it,
    /// when shapes answer every lookup from `object` to the holder. An own accessor's entry
    /// names no holder (`OWN_ACCESSOR`): a receiver of its shape holds its own accessor in
    /// that slot. Every prototype passed becomes a prototype first, so a later change to one
    /// advances `prototype_layout_epoch` and retires the entry.
    fn record_accessor_field_cache(&mut self, object: Value, atom: Atom, site: usize) {
        // The shared overflow site serves reads of every atom; an entry there would answer
        // one name's read with another's getter.
        if !self.field_cache_records(site) {
            return;
        }
        let epoch = crate::heap::prototype_layout_epoch();
        let Some(receiver) = self.shape_property_lookup(object, atom) else {
            return;
        };
        let (receiver_shape, prototype) = (receiver.shape(), receiver.proto());
        if let Some(slot) = self.shape_slot(receiver_shape, atom) {
            // An own accessor, as `debug`'s `enabled` on each logger function: every receiver
            // of the shape holds its own getter in the slot, read from it at each hit.
            let own_getter = slot < FIELD_CACHE_SLOT_CAPACITY
                && !receiver.has_side_descriptors()
                && self
                    .slot_attributes(receiver, slot)
                    .is_some_and(|attributes| attributes.accessor && attributes.getter.is_some());
            if own_getter {
                self.record_accessor_entry(
                    site,
                    AccessorFieldCache {
                        receiver: receiver_shape,
                        prototype,
                        holder: OWN_ACCESSOR,
                        slot: slot as u16,
                        epoch,
                    },
                );
            }
            return;
        }
        let mut link = prototype;
        let mut depth = 0u8;
        let (holder, slot) = loop {
            let Some(data) = self.heap.ordinary_object(link) else {
                return;
            };
            if data.is_arguments_object() || self.shape_property_lookup(link, atom).is_none() {
                return;
            }
            let Some(next_depth) = depth.checked_add(1) else {
                return;
            };
            depth = next_depth;
            if let Some(slot) = self.shape_slot(data.shape(), atom) {
                break (link, slot);
            }
            link = data.proto();
        };
        let Some(holder_object) = self.heap.ordinary_object(holder) else {
            return;
        };
        if slot >= FIELD_CACHE_SLOT_CAPACITY
            || !self
                .slot_attributes(holder_object, slot)
                .is_some_and(|attributes| attributes.accessor && attributes.getter.is_some())
        {
            return;
        }
        let mut link = prototype;
        for _ in 0..depth {
            self.heap.mark_prototype(link);
            link = self.heap.ordinary_object(link).map_or(Value::NULL, Object::proto);
        }
        // Marking a link that was no prototype yet leaves the epoch where it was.
        if crate::heap::prototype_layout_epoch() != epoch {
            return;
        }
        self.record_accessor_entry(
            site,
            AccessorFieldCache {
                receiver: receiver_shape,
                prototype,
                holder,
                slot: slot as u16,
                epoch,
            },
        );
    }

    /// Records `entry` among the site's accessor entries: one for its receiver shape is
    /// replaced, and a full site drops its oldest.
    fn record_accessor_entry(&mut self, site: usize, entry: AccessorFieldCache) {
        let entries = self.accessor_field_caches.entry(site).or_default();
        if let Some(existing) = entries.iter_mut().find(|cached| cached.receiver == entry.receiver) {
            *existing = entry;
            return;
        }
        if entries.len() >= INHERITED_FIELD_CACHE_WAYS {
            entries.remove(0);
        }
        entries.push(entry);
    }

    /// The getter a field read of `atom` at cache index `site` calls on `object`, when the
    /// site's recorded accessor still answers it (`record_accessor_field_cache`).
    pub(super) fn cached_field_getter(&self, object: Value, atom: Atom, site: usize) -> Option<Value> {
        let entries = self.accessor_field_caches.get(&site)?;
        let receiver = self.shape_property_lookup(object, atom)?;
        let epoch = crate::heap::prototype_layout_epoch();
        let cache = entries.iter().find(|cache| {
            receiver.shape() == cache.receiver && receiver.proto() == cache.prototype && epoch == cache.epoch
        })?;
        let holder = if cache.holder == OWN_ACCESSOR {
            if receiver.has_side_descriptors() {
                return None;
            }
            receiver
        } else {
            self.heap.ordinary_object(cache.holder)?
        };
        self.slot_attributes(holder, usize::from(cache.slot))?.getter
    }

    /// What native accessor `getter` returns for `this` where its call is a read of the
    /// receiver's internal slots that cannot throw for it, running no guest code and
    /// allocating nothing: a typed array's geometry and buffer, or a DataView's, Map's or
    /// Set's size accessors. `None`
    /// for any other getter or receiver, whose call the general path makes.
    pub(super) fn pure_native_getter_value(&self, getter: Value, this: Value) -> Option<Value> {
        let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = self.heap.function(getter) else {
            return None;
        };
        match *native {
            Native::TypedArrayLengthGetter
            | Native::TypedArrayByteLengthGetter
            | Native::TypedArrayByteOffsetGetter
            | Native::TypedArrayBufferGetter => self.typed_array_getter_value(*native, this),
            Native::DataViewByteLengthGetter | Native::DataViewByteOffsetGetter => {
                let (buffer, offset, length) = self.data_view_view(this)?;
                if self.array_buffer_out_of_bounds(buffer, offset, length) {
                    return None;
                }
                let value = if *native == Native::DataViewByteLengthGetter { length } else { offset };
                Some(Value::number(value as f64))
            }
            Native::MapSizeGetter => match self.heap.get(this) {
                Some(Cell::Map { entries, .. }) => Some(Value::number(entries.len() as f64)),
                _ => None,
            },
            Native::SetSizeGetter => match self.heap.get(this) {
                Some(Cell::Set { entries, .. }) => Some(Value::number(entries.len() as f64)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The part of `get_field_miss` that reads a data property of a shaped receiver or its
    /// ordinary prototypes, recording the cache entry that answers it next time; it runs no
    /// guest code. `None` leaves the read to `get_property`.
    pub(super) fn field_miss_data_value(
        &mut self,
        object: Value,
        atom: Atom,
        site: usize,
    ) -> Option<Value> {
        let receiver = self.shape_property_lookup(object, atom)?;
        let function_receiver = object.direct_object_index().is_none() && self.heap.function(object).is_some();
        if let Some((hit, proof)) = self.cached_inherited_field_value(site, receiver, function_receiver) {
            if let Some(proof) = proof {
                self.prove_inherited_chain(proof);
            }
            self.profile
                .field_cache_hit(usize::from(hit.tier), hit.depth);
            return Some(hit.value);
        }
        if self
            .property_attributes(object, PropertyKey::string(atom))
            .is_some_and(|attributes| attributes.accessor)
        {
            return None;
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
            return Some(value);
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
            return Some(value);
        }
        // A shape holds every own name of an ordinary object, every named own property of a
        // direct array (whose elements and virtual length the caller's shape lookup excludes),
        // and every own name `function_links_answer` admits of a function.
        let shape_holds_own = self.heap.ordinary_object(object).is_some()
            || self.heap.array(object).is_some()
            || self.heap.function(object).is_some() && self.function_links_answer(atom);
        let (cache, value) = self.inherited_field(receiver, shape_holds_own, atom)?;
        if self.field_cache_records(site) {
            self.note_field_cache_write(site);
            self.record_inherited_field(site, cache);
        }
        Some(value)
    }

    /// Records `cache` as the site's newest inherited entry. An entry for its receiver shape
    /// is replaced in place; a site holding `INHERITED_FIELD_CACHE_WAYS` entries reuses its
    /// oldest one's storage, so a site's entries stay bounded however many shapes reach it.
    fn record_inherited_field(&mut self, site: usize, mut cache: InheritedFieldCache) {
        let head = self.inherited_field_indices[site];
        let (mut index, mut previous, mut length) = (head, NO_INHERITED_FIELD, 0);
        while index != NO_INHERITED_FIELD {
            let entry = &self.inherited_field_caches[index as usize];
            if entry.receiver == cache.receiver {
                cache.next = entry.next;
                self.inherited_field_caches[index as usize] = cache;
                return;
            }
            length += 1;
            if entry.next == NO_INHERITED_FIELD {
                break;
            }
            (previous, index) = (index, entry.next);
        }
        let slot = if length >= INHERITED_FIELD_CACHE_WAYS {
            // The oldest entry, `index`, follows `previous`: more than one way keeps it apart
            // from the head the new entry links to.
            self.inherited_field_caches[previous as usize].next = NO_INHERITED_FIELD;
            index
        } else {
            self.inherited_field_caches.push(cache);
            (self.inherited_field_caches.len() - 1) as u32
        };
        cache.next = head;
        self.inherited_field_caches[slot as usize] = cache;
        self.inherited_field_indices[site] = slot;
    }

    /// An inherited data field cached by `inherited_field`: the receiver keeps its shape and
    /// each prototype up to the holder still has its recorded shape, which an entry proved under
    /// the current `prototype_layout_epoch` knows without the walk. Consulted on the miss path,
    /// so the inline cache tiers keep their shape. A walk's hit comes with the proof
    /// `prove_inherited_chain` records.
    #[inline(always)]
    fn cached_inherited_field_value(
        &self,
        site: usize,
        receiver: &Object,
        function_receiver: bool,
    ) -> Option<(FieldCacheHit, Option<InheritedChainProof>)> {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors; every other
        // index was installed together with its entry.
        let mut index = unsafe { *self.inherited_field_indices.get_unchecked(site) };
        let cache = loop {
            if index == NO_INHERITED_FIELD {
                return None;
            }
            let cache = unsafe { self.inherited_field_caches.get_unchecked(index as usize) };
            if cache.receiver == receiver.shape() {
                break cache;
            }
            index = cache.next;
        };
        if function_receiver && !cache.function_links {
            return None;
        }
        let hit = |value| FieldCacheHit {
            value,
            tier: 0,
            depth: u16::from(cache.depth),
        };
        let first = receiver.proto();
        let epoch = crate::heap::prototype_layout_epoch();
        if cache.proved_epoch == epoch && cache.proved_prototype == receiver.proto_link() {
            let value = match cache.answer {
                InheritedFieldAnswer::Slot(slot) => {
                    // A holder in the object records, as most prototypes are, without the
                    // out-of-line lookup of one in a slot.
                    let holder = self.heap.object(cache.proved_holder)?;
                    // SAFETY: the proof keeps the holder and its recorded shape.
                    unsafe { self.heap.property_get_shaped(holder, usize::from(slot)) }?
                }
                InheritedFieldAnswer::Absent => Value::UNDEFINED,
            };
            return Some((hit(value), None));
        }
        let mut prototype = first;
        let mut holder = None;
        let mut holder_value = Value::UNDEFINED;
        for &shape in &cache.prototype_shapes[..usize::from(cache.depth)] {
            let Some(data) = self.inherited_chain_link(prototype, cache.atom, cache.function_links) else {
                return None;
            };
            if data.shape() != shape || data.is_module_namespace() || data.is_arguments_object() {
                return None;
            }
            holder_value = prototype;
            prototype = data.proto();
            holder = Some(data);
        }
        let value = match cache.answer {
            // SAFETY: the holder's shape was recorded together with this data slot.
            InheritedFieldAnswer::Slot(slot) => unsafe {
                self.heap.property_get_shaped(holder?, usize::from(slot))
            }?,
            // Every recorded shape lacks the name, and the chain still ends here.
            InheritedFieldAnswer::Absent if prototype.is_null() => Value::UNDEFINED,
            InheritedFieldAnswer::Absent => return None,
        };
        let proof = InheritedChainProof {
            index,
            prototype: first,
            holder: holder_value,
            depth: cache.depth,
            epoch,
        };
        Some((hit(value), Some(proof)))
    }

    /// Records that a walk proved inherited entry `proof.index`'s chain under `proof.epoch`.
    /// Every link becomes a prototype first, so a later change to any of them advances the
    /// epoch and retires the proof.
    #[cold]
    #[inline(never)]
    fn prove_inherited_chain(&mut self, proof: InheritedChainProof) {
        let mut link = proof.prototype;
        for _ in 0..proof.depth {
            self.heap.mark_prototype(link);
            link = self.heap.object(link).map_or(Value::NULL, Object::proto);
        }
        if crate::heap::prototype_layout_epoch() != proof.epoch {
            return;
        }
        let cache = &mut self.inherited_field_caches[proof.index as usize];
        cache.proved_epoch = proof.epoch;
        cache.proved_prototype = crate::heap::ProtoLink::of(proof.prototype);
        cache.proved_holder = proof.holder;
    }

    /// Whether a function on a prototype chain answers `atom` from its shape as an ordinary
    /// object does, so an inherited entry for it may pass one: every name but its restricted
    /// ones (`caller`, `arguments`), the names exotic receivers answer outside their shape,
    /// the native-info names (`Vm::reads_function_info`) and `prototype`, whose slot may hold
    /// a constructor's prototype before it is made.
    fn function_links_answer(&self, atom: Atom) -> bool {
        !self
            .atom_class(atom)
            .intersects(AtomClass::RESTRICTED_FUNCTION_PROPERTY | AtomClass::VIRTUAL_FIELD)
            && !self.reads_function_info(atom)
            && self.known_prototype_atom() != Some(atom)
    }

    /// The object of a prototype-chain link an inherited entry passes, when its shape answers
    /// `atom` (`shape_property_lookup`); a function only where `function_links`
    /// (`function_links_answer`), as Node's classes inherit from their parent constructors
    /// (`Readable` from `Stream`).
    #[inline(always)]
    fn inherited_chain_link(&self, value: Value, atom: Atom, function_links: bool) -> Option<&Object> {
        if !function_links && self.heap.function(value).is_some() {
            return None;
        }
        self.shape_property_lookup(value, atom)
    }

    /// A data field of `atom` held more than one prototype above `object`, which lacks it,
    /// with every prototype on the way a non-dictionary object `inherited_chain_link` passes:
    /// the cache entry that answers the load again, and the value.
    #[cold]
    #[inline(never)]
    fn inherited_field(
        &self,
        object: &Object,
        shape_holds_own: bool,
        atom: Atom,
    ) -> Option<(InheritedFieldCache, Value)> {
        if self.shape_slot(object.shape(), atom).is_some() {
            return None;
        }
        let function_links = self.function_links_answer(atom);
        let mut cache = InheritedFieldCache {
            receiver: object.shape(),
            atom,
            prototype_shapes: [NO_FIELD_HOLDER; INHERITED_FIELD_CACHE_MAX_DEPTH],
            depth: 0,
            function_links,
            answer: InheritedFieldAnswer::Absent,
            proved_epoch: NO_PROVED_EPOCH,
            proved_prototype: crate::heap::ProtoLink::NULL,
            proved_holder: Value::UNDEFINED,
            next: NO_INHERITED_FIELD,
        };
        // Shapes alone prove a name absent only for a receiver whose shape holds every own
        // property of that name (`shape_holds_own`); an arguments object's indices and other
        // index names, and any exotic receiver's own answers, stay uncached.
        let absence_cacheable = shape_holds_own
            && !object.is_arguments_object()
            && !self.atom_class(atom).contains(AtomClass::ARRAY_INDEX);
        let mut prototype = object.proto();
        for depth in 0..INHERITED_FIELD_CACHE_MAX_DEPTH {
            if prototype.is_null() {
                return absence_cacheable.then_some((cache, Value::UNDEFINED));
            }
            let Some(data) = self.inherited_chain_link(prototype, atom, function_links) else {
                return None;
            };
            let shape = data.shape();
            if data.is_module_namespace()
                || data.is_arguments_object()
                || self.shape_is_dictionary(shape)
            {
                return None;
            }
            cache.prototype_shapes[depth] = shape;
            cache.depth = (depth + 1) as u8;
            if let Some(slot) = self.shape_slot(shape, atom) {
                // The immediate prototype is the regular field cache's holder tier.
                if depth == 0
                    || slot >= FIELD_CACHE_SLOT_CAPACITY
                    || self
                        .slot_attributes(data, slot)
                        .is_some_and(|attributes| attributes.accessor)
                {
                    return None;
                }
                cache.answer = InheritedFieldAnswer::Slot(slot as u16);
                return Some((cache, self.heap.property_get(data, slot)?));
            }
            prototype = data.proto();
        }
        (prototype.is_null() && absence_cacheable).then_some((cache, Value::UNDEFINED))
    }

    #[cold]
    #[inline(never)]
    fn immediate_prototype_data_field(
        &self,
        receiver_value: Value,
        atom: Atom,
    ) -> Option<(u32, usize, Value)> {
        let holder_value = self.object_data(receiver_value)?.proto();
        let holder = self.shape_property_lookup(holder_value, atom)?;
        let holder_shape = holder.shape();
        let slot = self.shape_slot(holder_shape, atom)?;
        if self
            .slot_attributes(holder, slot)
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

    /// `set_property`, then `set_property_attributes`: the host's way to install a property
    /// with chosen attributes. An absent data property is created with them in one shape.
    pub(super) fn set_property_with_attributes(
        &mut self,
        object: Value,
        atom: Atom,
        value: Value,
        attributes: PropertyAttributes,
    ) -> Result<(), JsError> {
        let key = PropertyKey::string(atom);
        if let Some(data) = DataAttributes::of(attributes)
            && let Some(object_data) = self.object_data(object)
            && self.property_shape_slot(object_data.shape(), key).is_none()
        {
            self.create_shape_data_property(object, key, value, data)?;
            if data != DEFAULT_DATA_ATTRIBUTES {
                self.invalidate_method_caches_for_key(key);
            }
            return Ok(());
        }
        self.set_property(object, atom, value)?;
        self.set_property_attributes(object, key, attributes);
        Ok(())
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
        if target == receiver && self.set_plain_named_property(target, atom, value).is_some() {
            return Ok(true);
        }
        let _stack = self.enter_stack()?;
        if self.is_private_name(atom) {
            self.check_private_brand(p, target, atom)?;
            if self.set_private_field_value(p, target, atom, value) { return Ok(true); }
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
            let held = if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
                && let Some(index) = super::object_static::array_index(self.atom_name(atom))
            {
                self.own_index_attributes(current, index as usize, Some(atom))
            } else {
                self.property_attributes(current, PropertyKey::string(atom))
                    .filter(|attributes| self.own_property(current, atom).is_some() || attributes.accessor)
            };
            if let Some(attributes) = held {
                found = Some((current, attributes));
                break;
            }
            let Some(data) = self.object_data(current) else {
                break;
            };
            current = data.proto();
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
            let key = self.heap.alloc_string(self.atom_value(atom));
            let descriptor = self.object_get_own_property_descriptor(p, &[receiver, key])?;
            if descriptor.is_undefined() {
                return Ok(false);
            }
            let current = self
                .descriptor_field(p, descriptor, self.runtime_atoms.value)?
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
            PropertyKey::String(atom) => self.heap.alloc_string(self.atom_value(atom)),
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
                if self.descriptor_field(p, current, self.runtime_atoms.get)?.is_some()
                    || self.descriptor_field(p, current, self.runtime_atoms.set)?.is_some()
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
        self.create_shape_data_property(object, key, value, DEFAULT_DATA_ATTRIBUTES)
    }

    pub(super) fn create_shape_data_property(
        &mut self,
        object: Value,
        key: PropertyKey,
        value: Value,
        attributes: DataAttributes,
    ) -> Result<(), JsError> {
        let shape = self
            .object_data(object)
            .ok_or_else(|| JsError("property write on non-object".into()))?
            .shape();
        self.check_property_key_write(object, key, false)?;
        let next_shape = self.transition_data_property_shape(shape, key, attributes);
        self.heap.property_add(object, value, next_shape);
        if let PropertyKey::String(atom) = key {
            self.invalidate_method_caches_for_prototype_add(object, atom);
        }
        Ok(())
    }

    #[inline(always)]
    pub(super) fn set_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
        strict: bool,
    ) -> Result<(), JsError> {
        if self.cached_field_store_hit(p, object, atom, value, site) {
            return Ok(());
        }
        self.set_field_polymorphic(p, object, atom, value, site, strict)
    }

    /// Stores `value` when it is an own-data store hit that no method cache can observe,
    /// which writes the cached slot in place and neither allocates nor throws. Adds,
    /// method-observable stores and misses return false for `set_field_polymorphic`.
    #[inline(always)]
    pub(super) fn cached_field_store_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> bool {
        if !(self.specialized && p.specialized) || self.method_caches_may_name(atom) {
            return false;
        }
        let Some(data) = self.heap.object(object) else {
            return false;
        };
        if data.is_module_namespace() || data.is_arguments_object() {
            return false;
        }
        let index = self.field_cache_index(site);
        let shape = data.shape();
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = unsafe { *self.field_caches.get_unchecked(index) };
        let slot = if cache.receiver == shape && cache.holder == NO_FIELD_HOLDER {
            Some(cache.slot)
        } else {
            self.inline_megamorphic_field_slot(index, shape)
                .or_else(|| self.hashed_megamorphic_own_field_slot(index, shape))
        };
        let Some(slot) = slot else {
            return false;
        };
        // SAFETY: a matching immutable shape proves the cached slot layout.
        unsafe {
            self.heap
                .property_set_unchecked(object, slot as usize, value)
        };
        self.profile.field_cache_hit(0, 0);
        true
    }

    /// Increment or decrement a cached own writable Number field without
    /// falling through the ordinary get/coerce/set bytecodes. The successful
    /// own-field load and strict-store caches prove the shape, data descriptor,
    /// and writability; the value is checked as Number before the store helper
    /// runs. Any miss leaves guest-visible state untouched.
    #[inline(always)]
    pub(super) fn cached_field_increment_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        read_site: u16,
        write_site: u16,
        decrement: bool,
    ) -> bool {
        let Some(old) = self.primary_own_field_hit(p, object, read_site) else {
            return false;
        };
        let Some(number) = old.as_number() else {
            return false;
        };
        let next = Value::number(if decrement { number - 1.0 } else { number + 1.0 });
        // A matching successful strict store cache proves this same shape has an own
        // writable data property at the field slot. The store helper rechecks that guard.
        self.cached_field_store_hit(p, object, atom, next, write_site)
    }

    /// A cached own-data store or a cached property add to an ordinary object: neither runs
    /// guest code nor throws, so the dispatch loop's fast match completes both.
    #[inline(always)]
    pub(super) fn cached_field_write_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> bool {
        self.cached_field_store_hit(p, object, atom, value, site)
            || self.cached_field_add_hit(p, object, atom, value, site)
    }

    /// The add half of `cached_field_write_hit`, out of line so stores keep their code.
    #[inline(never)]
    fn cached_field_add_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> bool {
        if !(self.specialized && p.specialized) || !self.field_add_atom_eligible(atom) {
            return false;
        }
        let Some(data) = self.heap.ordinary_object(object) else {
            return false;
        };
        if !data.admits_cached_add() {
            return false;
        }
        let (shape, link) = (data.shape(), data.proto_link());
        let site = self.field_cache_index(site);
        self.try_cached_field_add(object, atom, shape, link, value, site)
    }

    /// `set_field_cached` past the inline own-data store hits.
    #[inline(never)]
    fn set_field_polymorphic(
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
        if self.set_field_cache_hit(p, object, atom, value, site) {
            return Ok(());
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
            let key = self.atom_name(atom).to_owned();
            return Err(self.nullish_assignment_error(p, object, &key));
        }
        if self.is_private_name(atom) && self.set_private_field_value(p, object, atom, value) { return Ok(()); }
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
                self.proxy_set_mode(p, target, handler, object, PropertyKey::string(atom), value, strict)?;
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
                Err(self.primitive_assignment_error(p, object, atom))
            };
        }
        let own = self.own_property(object, atom).is_some();
        // Private initialization owns the receiver's hidden storage. A Proxy
        // in its public prototype chain cannot observe or mediate that write.
        if !own && !self.is_private_name(atom) && self.prototype_chain_contains_proxy(object) {
            return self.set_property_with_program_mode(p, object, atom, value, strict);
        }
        if let Some(attributes) = self.property_accessor(object, atom) {
            if let Some(setter) = attributes.setter {
                self.call_value(p, setter, object, &[value])?;
            } else if strict || self.is_private_name(atom) {
                return Err(self.rejected_assignment_error(p, object, atom, false));
            }
            return Ok(());
        }
        if own
            && self
                .property_attributes(object, PropertyKey::string(atom))
                .is_some_and(|attributes| !attributes.writable)
        {
            return if strict || self.is_private_name(atom) {
                Err(self.rejected_assignment_error(p, object, atom, false))
            } else {
                Ok(())
            };
        }
        if !own && self.inherited_write_blocked(object, atom) {
            return if strict || self.is_private_name(atom) {
                Err(self.rejected_assignment_error(p, object, atom, false))
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
                Err(self.rejected_assignment_error(p, object, atom, false))
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
            if existing.is_none() && shape != u32::MAX && self.field_add_atom_eligible(atom) {
                self.add_prototype_guard(object, PropertyKey::string(atom))
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
                && slot == self.shapes[shape as usize].storage_len()
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
        if !self.field_cache_records(site) {
            return;
        }
        let index = self.field_add_indices[site];
        if index == NO_FIELD_ADD_SET {
            self.note_field_cache_write(site);
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
        link: ProtoLink,
        value: Value,
        site: usize,
    ) -> bool {
        // The caller proved `object` an extensible ordinary object of `shape` whose prototype
        // `link` names.
        let Some((set, cache)) = self.field_add_entry(site, shape, atom) else {
            return false;
        };
        let (target_shape, slot) = (cache.target_shape, cache.slot);
        if !cache.proof_holds(link) && !self.prove_field_add_chain(set, shape, atom, link) {
            return false;
        }
        self.apply_cached_field_add(object, atom, shape, target_shape, slot, value);
        true
    }

    /// The add entry of cache site index `site` for receivers of `shape` adding `atom`, with
    /// the index of the site's set.
    #[inline(always)]
    fn field_add_entry(&self, site: usize, shape: u32, atom: Atom) -> Option<(usize, &FieldAddCache)> {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors; every non-sentinel
        // index was installed together with its set.
        let set = unsafe { *self.field_add_indices.get_unchecked(site) };
        if set == NO_FIELD_ADD_SET {
            return None;
        }
        let set = set as usize;
        Some((set, unsafe { self.field_add_sets.get_unchecked(set) }.find(shape, atom)?))
    }

    /// Adds `atom` to `object` of `shape` at `slot` by a cached transition to `target_shape`.
    #[inline(always)]
    fn apply_cached_field_add(
        &mut self,
        object: Value,
        atom: Atom,
        shape: u32,
        target_shape: u32,
        slot: u16,
        value: Value,
    ) {
        debug_assert_eq!(slot as usize, self.shapes[shape as usize].storage_len());
        self.heap.record_property_add_at(object, usize::from(slot), value, target_shape);
        if self.method_caches_may_name(atom) {
            self.invalidate_method_caches_for_prototype_add(object, atom);
        }
        self.profile.field_cache_hit(0, 0);
    }

    /// CreateDataPropertyOrThrow of the non-index key `atom` (a class field or literal
    /// property) through the site's add cache. A define never consults the prototype chain,
    /// so an entry needs only the source shape: the object's shape proves the key absent, and
    /// `class_field_define_target` proves no exotic own property or integrity level intervenes.
    pub(super) fn define_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> Result<(), JsError> {
        let key = PropertyKey::string(atom);
        let cacheable = self.specialized && p.specialized && self.field_cache_atom_eligible(atom);
        let Some(shape) = self.class_field_define_target(object).filter(|_| cacheable) else {
            return self.define_class_field(p, object, key, value);
        };
        let site = self.field_cache_index(site);
        if let Some((_, cache)) = self.field_add_entry(site, shape, atom) {
            let (target_shape, slot) = (cache.target_shape, cache.slot);
            debug_assert!(self.class_field_is_plain_add(object, key));
            self.apply_cached_field_add(object, atom, shape, target_shape, slot, value);
            return Ok(());
        }
        let plain = self.class_field_is_plain_add(object, key);
        self.define_class_field(p, object, key, value)?;
        let target_shape = self.object_data(object).map_or(shape, Object::shape);
        if plain
            && !self.shape_is_dictionary(shape)
            && !self.shape_is_dictionary(target_shape)
            && let Some(slot) = self.shape_slot(target_shape, atom)
            && slot == self.shapes[shape as usize].storage_len()
            && let Ok(slot) = u16::try_from(slot)
        {
            self.record_field_add_cache(
                site,
                FieldAddCache {
                    atom,
                    source_shape: shape,
                    target_shape,
                    slot,
                    ..FieldAddCache::EMPTY
                },
            );
        }
        Ok(())
    }

    /// The prototype shapes a cached add of `key` to `object` must find again, as the depth and
    /// shape fields of a `FieldAddCache`; `None` when a link is exotic, a dictionary, or holds
    /// `key` as an accessor or read-only.
    pub(super) fn add_prototype_guard(&self, object: Value, key: PropertyKey) -> Option<FieldAddCache> {
        let mut prototype = self.object_data(object)?.proto();
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
            prototype = data.proto();
        }
        Some(guard)
    }

    /// Whether the chain from the prototype `link` names still has the shapes the add entry
    /// of `shape` and `atom` recorded, which then holds that proof under the current
    /// `prototype_layout_epoch`. Every link becomes a prototype first, so a later change to
    /// any of them advances the epoch and retires the proof.
    #[cold]
    #[inline(never)]
    fn prove_field_add_chain(&mut self, set: usize, shape: u32, atom: Atom, link: ProtoLink) -> bool {
        let Some(cache) = self.field_add_sets[set].find(shape, atom) else {
            return false;
        };
        let (proto, depth) = (link.prototype(), cache.prototype_depth);
        let epoch = crate::heap::prototype_layout_epoch();
        // Entries record only non-dictionary source and prototype shapes, a shape's
        // dictionary state is fixed when it is created, and shape compaction clears these
        // caches, so a matching shape needs no dictionary probe here or along the chain.
        if !self.field_add_prototype_chain_matches(proto, cache.prototype_shapes()) {
            return false;
        }
        self.mark_prototype_links(proto, depth);
        // Marking a link that was no prototype yet leaves the epoch where it was, so the
        // proof made before it holds.
        if crate::heap::prototype_layout_epoch() == epoch
            && let Some(cache) = self.field_add_sets[set].find_mut(shape, atom)
        {
            cache.proved_epoch = epoch;
            cache.proved_prototype = link;
        }
        true
    }

    /// Marks the `depth` links of the chain from `proto` as prototypes, so a later change to
    /// any of them advances `prototype_layout_epoch`.
    pub(super) fn mark_prototype_links(&mut self, proto: Value, depth: u8) {
        let mut link = proto;
        for _ in 0..depth {
            self.heap.mark_prototype(link);
            link = self.heap.object_record(link).map_or(Value::NULL, Object::proto);
        }
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
            prototype = data.proto();
        }
        prototype.is_null()
    }

    #[inline(always)]
    /// Store through the field cache when it covers the write: a plain data
    /// object whose shape hits the site. Strictness only shapes failures, so
    /// a hit is the same store either way; a miss changes nothing.
    pub(super) fn set_field_cache_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> bool {
        if !(self.specialized && p.specialized) || self.is_private_name(atom) {
            return false;
        }
        let Some(data) = self.heap.object(object) else {
            return false;
        };
        if data.is_module_namespace() || data.is_arguments_object() {
            return false;
        }
        let site = self.field_cache_index(site);
        let (shape, link, extensible) = (data.shape(), data.proto_link(), data.is_extensible());
        self.try_cached_field_store(object, atom, shape, value, site)
            || (extensible
                && self.field_cache_atom_eligible(atom)
                && self.try_cached_field_add(object, atom, shape, link, value, site))
    }

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
            let key = self.atom_name(atom).to_owned();
            return Err(self.nullish_assignment_error(p, object, &key));
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
                Err(self.primitive_assignment_error(p, object, atom))
            };
        }
        let written = if let Some((target, handler)) = self.proxy_parts(object) {
            self.proxy_set_mode(p, target, handler, object, PropertyKey::string(atom), value, strict)?
        } else {
            self.set_property_with_receiver(p, object, atom, value, object)?
        };
        if written || !strict {
            Ok(())
        } else {
            Err(self.rejected_assignment_error(p, object, atom, false))
        }
    }

    pub(super) fn prototype_chain_contains_proxy(&self, object: Value) -> bool {
        let mut current = self.object_data(object).map(|data| data.proto());
        while let Some(value) = current.filter(|value| !value.is_null()) {
            if matches!(self.heap.get(value), Some(Cell::Proxy { .. })) {
                return true;
            }
            current = self.object_data(value).map(|data| data.proto());
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
            object = self.object_data(object)?.proto();
            if object.is_null() {
                return None;
            }
        }
    }

    pub(super) fn inherited_write_blocked(&self, object: Value, atom: Atom) -> bool {
        let mut object = self.object_data(object).map(|data| data.proto());
        while let Some(current) = object.filter(|value| !value.is_null()) {
            if let Some(attributes) = self.property_attributes(current, PropertyKey::string(atom)) {
                return !attributes.accessor && !attributes.writable;
            }
            object = self.object_data(current).map(|data| data.proto());
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
        if !self.field_cache_records(site) {
            return;
        }
        self.note_field_cache_write(site);
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
        self.transition_data_property_shape(shape, key, DEFAULT_DATA_ATTRIBUTES)
    }
    pub(super) fn transition_data_property_shape(
        &mut self,
        shape: u32,
        key: PropertyKey,
        attributes: DataAttributes,
    ) -> u32 {
        debug_assert!(
            self.walked_property_shape_slot(shape, key).is_none(),
            "a shape gains only keys it lacks"
        );
        let transition = ShapeTransition::Add {
            key,
            slot: u32::try_from(self.shapes[shape as usize].storage_len())
                .expect("object property index exceeds u32"),
            attributes,
        };
        self.append_cached_shape_transition(shape, transition)
    }
    pub(super) fn append_cached_shape_transition(&mut self, shape: u32, transition: ShapeTransition) -> u32 {
        let key = transition.cache_key();
        if let Some(key) = key {
            let last = self.shapes[shape as usize].last_child.get();
            let next = if last != NO_LAST_CHILD
                && super::shape_transitions::is_transition(&self.shapes, last, shape, key)
            {
                Some(last)
            } else {
                self.transitions.get(&self.shapes, shape, key)
            };
            if let Some(next) = next {
                self.shapes[shape as usize].last_child.set(next);
                self.profile.shape_transition(true);
                return next;
            }
        }
        if key.is_some() || matches!(transition, ShapeTransition::Add { .. }) {
            self.profile.shape_transition(false);
        }
        let storage_len = self.shapes[shape as usize].storage_len();
        let next_storage_len = match transition {
            ShapeTransition::Add { .. } => storage_len
                .checked_add(1)
                .filter(|length| u32::try_from(*length).is_ok())
                .expect("object property storage exhausted"),
            ShapeTransition::Descriptor { .. } | ShapeTransition::Delete { .. } | ShapeTransition::FunctionName { .. } | ShapeTransition::FunctionNameReconfigured | ShapeTransition::FunctionLengthReconfigured | ShapeTransition::Constructed { .. } => storage_len,
            ShapeTransition::Root
            | ShapeTransition::Vacant
            | ShapeTransition::Dictionary { .. } => {
                unreachable!("cached shape transitions are adds, deletions, descriptors or metadata")
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
            self.transitions.insert(&self.shapes, shape, key, next);
            self.shapes[shape as usize].last_child.set(next);
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
        let next_id = self.append_cached_shape_transition(
            shape,
            ShapeTransition::Delete {
                key,
                slot: slot as u32,
            },
        );
        // An object keeps a shape, which caches serve, until more of its slots are vacated
        // than hold properties: its storage is then mostly holes, and add-and-delete churn
        // (an object used as a map) would otherwise grow its shape chain without bound.
        let storage_len = self.shapes[next_id as usize].storage_len();
        let vacated = storage_len - self.shape_entries(next_id).len();
        let next_id = if self.shapes[next_id as usize].dictionary_trigger.is_none()
            && vacated > storage_len - vacated
        {
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
        self.reconfigure_function_name(object, key);
        self.invalidate_method_caches_for_key(key);
    }
}
