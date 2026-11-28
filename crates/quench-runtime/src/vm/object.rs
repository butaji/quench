use super::property_key::PropertyKey;
use super::dispatch_frame::this_is_fixed;
use super::*;
use crate::staged::field_cache_layout::{CLASS_KEY, field_index_of, is_class_key, is_spilled_slot, spilled_slot};
pub(super) use super::shape_lookup::derive_shape_lookup_index;
pub use super::field_cache::FieldCacheProbe;
use crate::value::HeapReference;
mod mru_chain;
mod shape_edits;
pub(super) use mru_chain::MRU_CHAIN_END;
use mru_chain::record_mru_entry;
/// The slot word of a stored field entry: the slot in the low half, and a store class in byte 2
/// (`general_store`).
pub const FIELD_BYTE: u32 = 0xff;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_host::TestHost;

    #[test]
    fn native_only_field_probes_use_semantic_specialization_permission() {
        let program = crate::Engine::specialize(r#"
            var own={value:7};
            function Static(){}
            Static.value=8;
            var inherited=Object.create({value:9});
            var getterCalls=0;
            var accessor={get value(){getterCalls++;return 11;}};
            function readOwn(object){return object.value;}
            function readCell(object){return object.value;}
            function readInherited(object){return object.value;}
            for(let i=0;i<4;i++){
                if(readOwn(own)!==7||readCell(Static)!==8||readInherited(inherited)!==9)throw 'data';
            }
        "#, "native-field-permission.js").unwrap();
        let native = crate::NativeJsProgram::publish(program).unwrap();
        let program = native.residual();
        let mut vm = Vm::new(TestHost::default());
        vm.set_execution_mode(crate::ExecutionMode::NativeOnly);
        vm.execute_native_program(&native, true).unwrap();
        assert!(vm.specialized_operations_enabled(program));
        assert!(!vm.admits_in_lane(crate::vm::ProgramId::MAIN, program), "native semantics do not admit interpreter lane dispatch");
        let site = |name: &str| {
            let function = program.functions.iter().position(|body| body.header().name
                .is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
            program.function(function).instructions().find_map(|instruction| match instruction.field_lookup() {
                Some(crate::bytecode::FieldLookup::Atom { atom, cache_site, .. })
                    if &program.atoms[atom as usize] == "value" => Some(cache_site),
                _ => None,
            }).unwrap()
        };
        let mut global = |name| {
            let atom = vm.intern_atom(name);
            vm.own_property(vm.realm.globals, atom).unwrap()
        };
        let own = global("own");
        let function = global("Static");
        let inherited = global("inherited");
        let accessor = global("accessor");
        let own_site = site("readOwn");
        let cell_site = site("readCell");
        let inherited_site = site("readInherited");
        assert_eq!(vm.primary_own_field_hit(program, own, own_site), Some(Value::integer(7)));
        assert_eq!(vm.cell_own_field_hit(program, function, cell_site), Some(Value::integer(8)));
        assert_eq!(vm.primary_holder_field_hit(program, inherited, inherited_site), Some(Value::integer(9)));
        assert_eq!(vm.primary_record_holder_field_hit(program, inherited, inherited_site), Some(Value::integer(9)));
        assert!(matches!(vm.cached_field_hit(program, function, cell_site), FieldCacheProbe::Hit(value)
            if value == Value::integer(8)));
        assert_eq!(vm.cell_own_field_hit(program, accessor, own_site), None,
            "a data probe must leave accessor semantics untouched");
        let value = vm.intern_atom("value");
        assert_eq!(vm.get_field_cached(program, accessor, value, own_site).unwrap(), Value::integer(11));
        let calls = vm.intern_atom("getterCalls");
        assert_eq!(vm.own_property(vm.realm.globals, calls), Some(Value::integer(1)));
        vm.specialized = false;
        assert!(!vm.specialized_operations_enabled(program));
        assert!(matches!(vm.cached_field_hit(program, own, own_site), FieldCacheProbe::Ineligible));
    }
}

const PRIVATE_NAME_PREFIX: &str = "\0quench:private:";

/// Bit set of facts derived from an atom's text; `DERIVED` marks the set as computed.
#[derive(Clone, Copy)]
pub struct AtomClass(u8);

impl AtomClass {
    const DERIVED: u8 = 1 << 0;
    pub const PRIVATE: u8 = 1 << 1;
    pub const ARRAY_INDEX: u8 = 1 << 2;
    pub(super) const TYPED_ARRAY_INDEX: u8 = 1 << 3;
    pub(super) const RESTRICTED_FUNCTION_PROPERTY: u8 = 1 << 4;
    /// Names that exotic or accessor-backed built-in receivers (array `length`, collection
    /// `size`, buffer and view geometry) answer outside ordinary shape storage, so field
    /// caches never record them.
    pub const VIRTUAL_FIELD: u8 = 1 << 5;
    /// The compiler's hidden name for a named function expression's own immutable binding.
    pub(super) const SELF_BINDING: u8 = 1 << 6;
    /// Engine-internal names, which begin with NUL and never name a source binding.
    pub const INTERNAL: u8 = 1 << 7;
    /// The names whose answer a holder's exotic kind or array and typed-array index rules
    /// decide, beyond its shape: `shape_property_lookup` excludes them per receiver family, so
    /// a field cache hit reads its holder through the lookup for them (`non_record_hit_holder`).
    pub(super) const FAMILY_SENSITIVE: u8 =
        Self::PRIVATE | Self::ARRAY_INDEX | Self::TYPED_ARRAY_INDEX | Self::RESTRICTED_FUNCTION_PROPERTY | Self::VIRTUAL_FIELD;

    #[inline(always)]
    pub fn contains(self, bits: u8) -> bool {
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
pub(super) const SHAPE_INDEX_WALK_DISTANCE: usize = 32;
/// Long walks from one shape after which that shape gets a lookup index.
pub(super) const SHAPE_INDEX_LONG_WALKS: u8 = 8;
pub(super) const FIELD_CACHE_SLOT_CAPACITY: usize = u16::MAX as usize + 1;
/// `AccessorFieldCache::holder` of an accessor the receiver holds itself.
pub(super) const OWN_ACCESSOR: Value = Value::UNDEFINED;

struct FieldCacheHit {
    value: Value,
    tier: u8,
    depth: u16,
}

/// Which of a polymorphic site's entries answered a read (`Vm::hinted_polymorphic_field_hit`).
#[derive(Clone, Copy)]
pub enum PolymorphicHit {
    /// The own entry the reading record keeps as its hint.
    Hint,
    /// Another own entry, of the receiver's exact shape.
    Own(FieldCache),
    /// An entry of the receiver's immediate prototype, of both exact shapes.
    Holder(FieldCache),
}

/// What a walk of an inherited entry's chain proved (`Vm::prove_inherited_chain`).
#[derive(Clone, Copy)]
struct InheritedChainProof {
    index: u32,
    prototype: Value,
    holder: Value,
    depth: u8,
}

enum FieldCacheRead {
    Proxy,
    Generic,
    StringGeneric,
    Miss(usize),
    StringMiss { prototype: Value, site: usize },
    Hit(FieldCacheHit),
}

/// A census of shape creations by transition key (`QUENCH_TRACE=shapes`, `profile-memory`):
/// which operations keep making a shape instead of finding it in the cached child index.
#[cfg(feature = "profile-memory")]
mod shape_trace {
    use std::sync::Mutex;
    use std::sync::OnceLock;

    static CREATED: OnceLock<Mutex<std::collections::HashMap<String, u64>>> = OnceLock::new();

    pub(super) fn record(label: String) {
        let created = CREATED.get_or_init(|| {
            // SAFETY: `report` is a plain function that outlives the process.
            unsafe { libc::atexit(report) };
            Mutex::new(std::collections::HashMap::new())
        });
        if let Ok(mut created) = created.lock() {
            *created.entry(label).or_default() += 1;
        }
    }

    extern "C" fn report() {
        let Some(created) = CREATED.get() else { return };
        let Ok(created) = created.lock() else { return };
        let mut rows: Vec<_> = created.iter().collect();
        rows.sort_unstable_by(|left, right| right.1.cmp(left.1));
        eprintln!("QUENCH_SHAPE_TRACE created={}", rows.iter().map(|(_, count)| **count).sum::<u64>());
        for (label, count) in rows.iter().take(20) {
            eprintln!("  shape {count:>9} {label}");
        }
    }
}

impl Vm {
    /// Name-derived property classes, computed from the atom text once per atom.
    #[inline(always)]
    pub fn atom_class(&self, atom: Atom) -> AtomClass {
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
    pub fn is_private_name(&self, atom: Atom) -> bool {
        self.atom_class(atom).contains(AtomClass::PRIVATE)
    }

    #[inline(always)]
    pub fn shape_slot(&self, shape: u32, atom: Atom) -> Option<usize> {
        self.property_shape_slot(shape, PropertyKey::string(atom))
    }
    #[inline(always)]
    pub(super) fn property_shape_slot(&self, shape: u32, key: PropertyKey) -> Option<usize> {
        // A shape's lookup index, once it has one, is the home of its key -> slot facts; the
        // memo stands in only for index-less shapes, so a wide object's keys never evict them.
        let entry = &self.shapes[shape as usize];
        if let Some(index) = entry.lookup_index.get() {
            return index.slot(key).map(|slot| slot as usize);
        }
        if entry.is_dictionary() {
            return self.shape_lookup_index(shape).slot(key).map(|slot| slot as usize);
        }
        if let Some(slot) = self.shape_slot_memo.get(shape, key) {
            return slot.map(|slot| slot as usize);
        }
        let slot = self.walked_property_shape_slot(shape, key);
        // A slot past u32 is never memoized; it is walked again.
        if let Ok(recorded) = slot.map(u32::try_from).transpose() {
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
                ShapeTransition::FunctionName { .. }
                | ShapeTransition::FunctionNameReconfigured
                | ShapeTransition::FunctionLengthReconfigured
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
    pub(super) fn note_long_shape_walk(&self, shape: u32) {
        let entry = &self.shapes[shape as usize];
        let walks = entry.long_walks() + 1;
        if walks < SHAPE_INDEX_LONG_WALKS {
            entry.set_long_walks(walks);
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
        let shape = self.heap.object(object)?.shape();
        Some((shape, self.property_shape_slot(shape, key)?))
    }
    /// The attributes of `object`'s shape-held property at `slot`, its getter read from the
    /// slot when the shape keeps it there (`SLOT_GETTER`).
    pub(super) fn slot_attributes<'a>(&self, object: impl Into<ObjectRef<'a>>, slot: usize) -> Option<PropertyAttributes> {
        let object: ObjectRef<'a> = object.into();
        let attributes = self.shape_slot_attributes(object.shape(), slot)?;
        Some(attributes.attributes(&self.shape_accessors, || {
            self.slot_held_accessors(self.heap.property_get_ref(object, slot))
        }))
    }
    /// Whether a raw slot read of an object whose shape chain made an accessor with a setter
    /// (`ShapeFacts::ACCESSOR_PAIRS`) met a pair cell (`SLOT_ACCESSOR_PAIR`) rather than a
    /// value: only such a slot holds one, and a value's tag rules out most others at once.
    #[inline(always)]
    pub(super) fn is_accessor_pair(&self, shape: u32, value: Value) -> bool {
        self.shapes[shape as usize].facts.accessor_pairs()
            && matches!(value.heap_reference(), Some(crate::value::HeapReference::LegacySlot(_)))
            && matches!(self.heap.get(value), Some(Cell::AccessorPair { .. }))
    }
    /// The accessor functions an accessor's slot `value` holds (`ShapeAttributes::slot_accessors`):
    /// those of its pair cell, else the getter it is.
    pub(super) fn slot_held_accessors(&self, value: Option<Value>) -> super::ShapeAccessors {
        match value.and_then(|value| self.heap.get(value)) {
            Some(Cell::AccessorPair { getter, setter }) => (Some(*getter), Some(*setter)),
            _ => (value, None),
        }
    }
    /// The value an accessor's slot holds for `attributes` recorded as `recorded`: its getter
    /// (`SLOT_GETTER`), a new pair cell of its getter and setter (`SLOT_ACCESSOR_PAIR`), or
    /// `undefined` where the shape's accessor table holds them.
    fn slot_accessor_value(&mut self, attributes: PropertyAttributes, recorded: super::ShapeAttributes) -> Value {
        if recorded.slot_getter() {
            attributes.getter.expect("a slot getter record has a getter")
        } else if recorded.slot_accessor_pair() {
            self.heap.alloc(Cell::AccessorPair {
                getter: attributes.getter.expect("a slot pair record has a getter"),
                setter: attributes.setter.expect("a slot pair record has a setter"),
            })
        } else {
            Value::UNDEFINED
        }
    }
    /// `object`'s data value at `slot`, as a raw read sees it: `undefined` for an accessor,
    /// whose slot holds its getter or nothing.
    pub(super) fn slot_data_value<'a>(&self, object: impl Into<ObjectRef<'a>>, slot: usize) -> Option<Value> {
        let object: ObjectRef<'a> = object.into();
        let value = self.heap.property_get_ref(object, slot)?;
        let shape = object.shape();
        if self.shapes[shape as usize].facts.slot_getters()
            && self
                .shape_slot_attributes(shape, slot)
                .is_some_and(ShapeAttributes::slot_getter)
            || self.is_accessor_pair(shape, value)
        {
            return Some(Value::UNDEFINED);
        }
        Some(value)
    }
    pub(super) fn shape_slot_attributes(&self, shape: u32, slot: usize) -> Option<ShapeAttributes> {
        if let Some(index) = self.shapes[shape as usize].lookup_index.get() {
            return Some(index.attributes.get(&(slot as u32)).copied().unwrap_or(DEFAULT_SHAPE_ATTRIBUTES));
        }
        let layout = &self.shapes[shape as usize];
        if layout.facts.default_attributes() && !self.shape_is_dictionary(shape) {
            return (slot < layout.storage_len()).then_some(DEFAULT_SHAPE_ATTRIBUTES);
        }
        // A non-dictionary shape never changes, so its slot's attributes are a fact of the
        // pair: memoized, a wide accessor object (mobx observables) does not walk its chain
        // at every accessor read.
        if self.shape_is_dictionary(shape) {
            return self.walked_shape_attribute(shape, slot);
        }
        let Ok(key) = u32::try_from(slot) else { return self.walked_shape_attribute(shape, slot) };
        if let Some(attributes) = self.shape_attribute_memo.get(shape, key) {
            return attributes;
        }
        let attributes = self.walked_shape_attribute(shape, slot);
        self.shape_attribute_memo.record(shape, key, attributes);
        attributes
    }
    pub fn shape_keys(&self, shape: u32) -> Vec<PropertyKey> {
        self.shape_entries(shape)
            .map(|(key, _)| key)
            .collect()
    }
    /// Moves `parent`'s lookup index to `child`, a cached transition of it the object took, as
    /// `append_shape` moves it to a new child. Otherwise the index stays with the shape that
    /// built it and an object retracing the chain derives one per shape from scratch: the
    /// second wide object of one layout (a 150K-unit string's wrapper, SunSpider tagcloud)
    /// cost time and index memory quadratic in its width.
    fn carry_lookup_index(&mut self, parent: u32, child: u32) {
        // The parent's index leaves it either way. Kept on the parent beside a child that
        // already has one, it stays for good: every later build of the same wide layout
        // (an object filled key by key each run) strands one more chain-sized index one
        // shape further up (SunSpider string-unpack-code: +0.23 MB per iteration, peak RSS
        // linear in the run length). Dropping it keeps at most one index per walk.
        let Some(index) = self.shapes[parent as usize].lookup_index.take() else {
            return;
        };
        if self.shapes[child as usize].lookup_index.get().is_some() {
            return;
        }
        let mut steps = smallvec::SmallVec::<[ShapeTransition; 2]>::new();
        let mut current = child;
        while current != parent {
            let shape = &self.shapes[current as usize];
            steps.push(shape.transition);
            current = shape.parent().expect("a cached transition descends from its parent");
        }
        if let Some(index) = steps.into_iter().rev().try_fold(index, |index, step| index.extended_by(step)) {
            self.shapes[child as usize].lookup_index = OnceCell::from(index);
        }
    }
    pub(super) fn append_shape(
        &mut self,
        parent: u32,
        transition: ShapeTransition,
        storage_len: usize,
    ) -> u32 {
        #[cfg(feature = "profile-memory")]
        if crate::trace::Channel::Shapes.on() {
            // Which transitions keep missing the cached child index, by key: a shape a
            // repeated operation re-creates names the key that fails to dedupe.
            let label = match &transition {
                ShapeTransition::Add { key, .. } => match key {
                    property_key::PropertyKey::String(atom) => format!("Add:{}", self.atom_name(*atom)),
                    property_key::PropertyKey::Symbol(_) => "Add:<symbol>".to_owned(),
                    property_key::PropertyKey::Private(..) => "Add:<private>".to_owned(),
                },
                ShapeTransition::Delete { key, .. } => match key {
                    property_key::PropertyKey::String(atom) => format!("Delete:{}", self.atom_name(*atom)),
                    _ => "Delete:<nonstring>".to_owned(),
                },
                ShapeTransition::Descriptor { slot, .. } => format!("Descriptor:{slot}"),
                ShapeTransition::Dictionary { .. } => "Dictionary".to_owned(),
                ShapeTransition::Root => "Root".to_owned(),
                ShapeTransition::Vacant => "Vacant".to_owned(),
                ShapeTransition::FunctionName { .. } => "FunctionName".to_owned(),
                ShapeTransition::FunctionNameReconfigured => "FunctionNameReconfigured".to_owned(),
                ShapeTransition::FunctionLengthReconfigured => "FunctionLengthReconfigured".to_owned(),
                ShapeTransition::Constructed { .. } => "Constructed".to_owned(),
            };
            shape_trace::record(label);
        }
        let next = u32::try_from(self.shapes.len()).expect("object shape table exhausted");
        let dictionary_trigger = match transition {
            ShapeTransition::Dictionary { trigger } => Some(trigger),
            _ => self.shapes[parent as usize].dictionary_trigger(),
        };
        let parent_facts = self.shapes[parent as usize].facts;
        // The object that grew has moved to the child, so the parent's lookup index moves
        // with it, extended by the transition, and so do the long walks it has spent toward
        // one; an object left on the parent rebuilds it after `SHAPE_INDEX_LONG_WALKS` long
        // walks. Otherwise every intermediate shape of a growing wide object (the global
        // object during bootstrap) kept a full index, or each one walked its whole chain anew.
        let long_walks = self.shapes[parent as usize].long_walks();
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
        child.set_long_walks(long_walks);
        if let Some(index) = lookup_index {
            child.lookup_index = OnceCell::from(index);
        }
        push_child(&mut self.shapes, child);
        let parent_length = self.shapes[parent as usize].storage_len();
        self.heap.register_property_shape(next, storage_len, Some((parent, parent_length)));
        next
    }
    /// The capacity an add that moves an object's arena run to `shape` reserves for a run of
    /// `length` values (the new length): the size the shape tree predicts. It follows the only
    /// child of each shape while that child is an `Add` of one more slot, and takes the length
    /// the chain reaches, clamped to `length..=PREDICTED_RUN_CAP_FACTOR * length`, as a size
    /// class. Walked only on a move, which already copies the run, never on an add that stays.
    pub(super) fn predicted_run_capacity(&self, shape: u32, length: usize) -> usize {
        let cap = length * crate::value_vec::PREDICTED_RUN_CAP_FACTOR;
        let mut extent = length;
        let mut cursor = shape;
        while extent < cap {
            let entry = &self.shapes[cursor as usize];
            if entry.facts.children() != 1 || entry.is_dictionary() {
                break;
            }
            let child_id = entry.last_child.get();
            let Some(child) = self.shapes.get(child_id as usize) else {
                break;
            };
            if child.parent() != Some(cursor)
                || !matches!(child.transition, ShapeTransition::Add { .. })
                || child.storage_len() != entry.storage_len() + 1
            {
                break;
            }
            extent = child.storage_len();
            cursor = child_id;
        }
        crate::value_vec::capacity_class(extent.clamp(length, cap))
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

    pub fn shape_is_dictionary(&self, shape: u32) -> bool {
        self.shapes[shape as usize].is_dictionary()
    }
    /// Whether a property of `shape` may be an accessor: a dictionary shape keeps its
    /// attributes outside its transitions, any other knows from its chain (`ShapeFacts`).
    #[inline(always)]
    pub fn shape_may_have_accessors(&self, shape: u32) -> bool {
        let entry = &self.shapes[shape as usize];
        entry.is_dictionary() || entry.facts.accessors()
    }
    /// Whether a property of `shape` may be an accessor with a setter (`ShapeFacts::ACCESSOR_PAIRS`):
    /// a dictionary shape keeps its attributes outside its transitions, any other knows from
    /// its chain.
    #[inline(always)]
    pub(super) fn shape_may_have_accessor_pairs(&self, shape: u32) -> bool {
        let entry = &self.shapes[shape as usize];
        entry.is_dictionary() || entry.facts.accessor_pairs()
    }
    /// Whether a property of `shape` may be keyed by a symbol or private name (`ShapeFacts::HAS_SYMBOL_KEY`):
    /// a dictionary shape keeps its keys outside its transitions, any other knows from its chain.
    #[inline(always)]
    pub(super) fn shape_may_have_symbol_keys(&self, shape: u32) -> bool {
        let entry = &self.shapes[shape as usize];
        entry.facts.is_dictionary() || entry.facts.has_symbol_key()
    }
    pub(super) fn mark_object_dictionary(&mut self, object: Value, trigger: DictionaryTrigger) {
        // A class cell stays one (departure-free S3 and S4), whatever the trigger; a record takes the shape in place.
        let cell = self.heap.live_class_cell(object);
        let Some(shape) = self.heap.object(object).map(|object| object.shape()) else {
            return;
        };
        if self.shape_is_dictionary(shape) {
            return;
        }
        let storage_len = self.shapes[shape as usize].storage_len();
        let dictionary_shape =
            self.append_shape(shape, ShapeTransition::Dictionary { trigger }, storage_len);
        match cell {
            Some(granule) => self.heap.cell_to_dictionary(granule, dictionary_shape),
            None => self.heap.set_object_shape(object, dictionary_shape),
        }
        self.profile.dictionary_transition(trigger);
    }
    #[inline(always)]
    pub fn property_attributes(&self, object: Value, key: PropertyKey) -> Option<PropertyAttributes> {
        let slot = self.heap.object(object).and_then(|data| self.property_shape_slot(data.shape(), key));
        self.property_attributes_at(object, key, slot)
    }

    /// `object`'s own shape slot for `atom`: the slot `property_attributes` and `own_property`
    /// both resolve from its shape. A caller that reads both resolves it once and passes it to
    /// `property_attributes_at` and `own_property_at`.
    #[inline(always)]
    pub(super) fn own_shape_slot(&self, object: Value, atom: Atom) -> Option<usize> {
        self.heap.object(object).and_then(|data| self.shape_slot(data.shape(), atom))
    }

    /// `property_attributes` of `key` on `object` given its shape `slot` (`own_shape_slot`).
    #[inline(always)]
    pub(super) fn property_attributes_at(
        &self,
        object: Value,
        key: PropertyKey,
        slot: Option<usize>,
    ) -> Option<PropertyAttributes> {
        if let Some(slot) = slot
            && let Some(attributes) = self.heap.object(object).and_then(|data| self.slot_attributes(data, slot))
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
        .or_else(|| self.string_wrapper_index(object, key).map(|_| super::string_wrapper::STRING_INDEX_ATTRIBUTES))
    }
    /// `object`'s attributes for `key` in the descriptor table, which an object only has
    /// entries in once marked (`Object::has_side_descriptors`); any other value is looked up.
    #[inline(always)]
    pub(super) fn side_descriptor(&self, object: Value, key: PropertyKey) -> Option<PropertyAttributes> {
        // record-only: side descriptors
        if self.heap.object(object).is_some_and(|data| !data.has_side_descriptors()) {
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
        // The marks are the object's element facts: a cell's live in its extras, made here (`ensure_cell_extras`).
        self.heap.ensure_cell_extras(object);
        if let Some(mut data) = self.heap.object_mut(object) {
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
        let named =
            atom.and_then(|atom| self.property_attributes(holder, PropertyKey::string(atom)));
        if self.own_element_present(holder, index) {
            let attributes = named.unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            let frozen = self.heap.object(holder).is_some_and(|object| object.is_frozen());
            return Some(PropertyAttributes {
                writable: attributes.writable && !frozen,
                ..attributes
            });
        }
        let held = self.string_wrapper_length(holder).is_some_and(|length| index < length)
            || atom.is_some_and(|atom| self.own_property(holder, atom).is_some());
        named.filter(|attributes| held || attributes.accessor)
    }

    /// Whether `holder` provably has no own property at array index `index` without naming
    /// it: no element there, no index-keyed descriptor and no index name among its shape's
    /// keys.
    fn lacks_own_index(&self, holder: Value, index: usize) -> bool {
        self.heap.object(holder).is_some_and(|data| self.object_lacks_own_index(holder, data, index))
    }

    /// `lacks_own_index` of a holder whose object `data` the caller has read.
    #[inline(always)]
    fn object_lacks_own_index<'a>(&self, holder: Value, data: impl Into<ObjectRef<'a>>, index: usize) -> bool {
        let data: ObjectRef<'a> = data.into();
        !data.has_indexed_descriptors()
            && !self.shape_has_index_key(data.shape())
            && !self.own_element_present(holder, index)
            && (data.boxed_primitive().is_none() || self.string_wrapper_length(holder).is_none_or(|length| index >= length))
    }

    /// Whether any key of `shape` is an array-index name, memoized on the immutable shape
    /// from its nearest known ancestor. A dictionary shape answers conservatively.
    pub(super) fn shape_has_index_key(&self, shape: u32) -> bool {
        if self.shape_is_dictionary(shape) {
            return true;
        }
        if let Some(known) = self.shapes[shape as usize].index_keys() {
            return known;
        }
        let mut current = Some(shape);
        let found = loop {
            let Some(id) = current else {
                break false;
            };
            let candidate = &self.shapes[id as usize];
            if let Some(known) = candidate.index_keys() {
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
        self.shapes[shape as usize].set_index_keys(found);
        found
    }

    /// Whether no prototype on the chain from `first` holds an array index: answered by the
    /// held proof (`IndexFreeChainProof`), which a change to any prototype it read retires.
    #[inline]
    pub(super) fn chain_holds_no_index(&self, first: Value) -> bool {
        self.heap.dependencies().index_free_chain().is_some_and(|proof| proof.first == first || proof.next == first)
            || self.prove_index_free_chain(first)
    }

    /// Whether array `array` has no element, dense or sparse.
    fn array_holds_no_element(&self, array: Value) -> bool {
        self.heap.array_elements(array).is_none_or(|elements| elements.is_empty())
            && self.heap.sparse_length(array).is_none()
    }

    /// `chain_holds_no_index` past its proof: the walk, recording the proof on each
    /// prototype it read when it holds and every one is watched (`Heap::mark_prototype`),
    /// so a change to any of them retires it.
    #[cold]
    #[inline(never)]
    pub(super) fn prove_index_free_chain(&self, first: Value) -> bool {
        let mut subjects = Vec::new();
        let mut array_holder = false;
        let mut current = first;
        while !current.is_null() {
            let Some(data) = self.heap.object(current) else {
                return false;
            };
            let is_array = self.heap.array(current).is_some();
            if self.may_hold_index(current, data, is_array) {
                return false;
            }
            if is_array {
                // One array holder is proved empty; a chain of more is not proved.
                if array_holder || !self.array_holds_no_element(current) {
                    return false;
                }
                array_holder = true;
            }
            subjects.push(data.watched_subject());
            current = data.proto();
        }
        if let Some(subjects) = subjects.into_iter().collect::<Option<Vec<u32>>>() {
            let next = self.heap.object(first).map_or(Value::NULL, |data| data.proto());
            let proof = crate::heap::IndexFreeChainProof::new(first, next);
            // The link word mirrors the held proof only where that proof covers `Array.prototype`'s
            // chain. Recording any other proof replaces a held one, so the word clears then.
            let link = if first == self.array_prototype_for_realm(self.realm.globals) {
                u64::from(proof.links[0])
            } else {
                crate::heap::NO_INDEX_FREE_CHAIN_LINK
            };
            self.heap.dependencies().prove(crate::heap::ProofKind::IndexFreeChain, &subjects, |proofs| {
                proofs.index_free_chain = Some(proof);
                crate::heap::set_index_free_chain_link(link);
            });
        }
        true
    }

    /// Whether `object` (its `data`) may answer an index key itself: indexed descriptors, an
    /// exotic or boxed-primitive object, an index key in its shape, or no ordinary object.
    fn may_hold_index<'a>(&self, object: Value, data: impl Into<ObjectRef<'a>>, is_array: bool) -> bool {
        let data: ObjectRef<'a> = data.into();
        data.has_indexed_descriptors()
            || data.is_arguments_object()
            || data.boxed_primitive().is_some()
            || data.is_module_namespace()
            || data.deferred_module().is_some()
            || self.shape_has_index_key(data.shape())
            || (!is_array && self.heap.ordinary_view(object).is_none())
    }

    /// Whether OrdinarySet of array index `index` on `object`, which holds no own property
    /// there, ends in a plain data definition on `object`: no prototype answers it with an
    /// accessor, read-only data or an exotic [[Set]] (proxy, typed array, module namespace).
    pub(super) fn inherited_index_unclaimed(&self, object: Value, index: usize) -> bool {
        let first = self.heap.object(object).map(|data| data.proto());
        if first.is_some_and(|first| self.chain_holds_no_index(first)) {
            return true;
        }
        let mut atom = None;
        let mut current = first;
        while let Some(holder) = current.filter(|value| !value.is_null()) {
            // A direct object or array record is neither a Proxy nor a typed array, whose
            // cells only a slot holds.
            let record = holder.direct_object_index().is_some() || holder.direct_array_index().is_some();
            if !record
                && matches!(
                    self.heap.get(holder),
                    Some(Cell::Proxy { .. } | Cell::TypedArray { .. })
                )
            {
                return false;
            }
            let Some(data) = self.heap.object(holder) else {
                return false;
            };
            if data.is_module_namespace() {
                return false;
            }
            if !self.object_lacks_own_index(holder, data, index) {
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
        let mut current = self.heap.object(object).map(|data| data.proto());
        while let Some(holder) = current.filter(|value| !value.is_null()) {
            if matches!(self.heap.get(holder), Some(Cell::Proxy { .. } | Cell::TypedArray { .. })) {
                return false;
            }
            let Some(data) = self.heap.object(holder) else {
                return false;
            };
            if data.has_exotic_access()
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
    /// data definition (`inherited_index_unclaimed`). A single index (a push) is checked
    /// alone, which costs less than the chain-wide proof; a wider range proves the chain once.
    pub(super) fn array_indices_store_plainly(
        &self,
        array: Value,
        mut indices: std::ops::Range<usize>,
    ) -> bool {
        if indices.len() > 1 && self.prototypes_hold_no_index(array) {
            return true;
        }
        indices.all(|index| self.inherited_index_unclaimed(array, index))
    }

    /// Whether no prototype of `object` holds any array index or could answer one outside its
    /// storage: OrdinarySet of an index `object` lacks then defines it on `object`
    /// (`inherited_index_unclaimed`), and HasProperty of it finds nothing inherited
    /// (`inherited_index_absent`), whichever the index.
    pub fn prototypes_hold_no_index(&self, object: Value) -> bool {
        let first = self.heap.object(object).map(|data| data.proto());
        self.chain_from_holds_no_index(first)
    }

    /// `prototypes_hold_no_index` of the chain that starts at `first`, the prototype of the object
    /// being checked; `None` where that object has no record.
    pub(super) fn chain_from_holds_no_index(&self, first: Option<Value>) -> bool {
        // The held proof answers a chain it covers without the walk.
        if first.is_some_and(|first| self.chain_holds_no_index(first)) {
            return true;
        }
        let mut current = first;
        while let Some(holder) = current.filter(|value| !value.is_null()) {
            // A direct object or array record is neither a Proxy nor a typed array, whose
            // cells only a slot holds.
            let record = holder.direct_object_index().is_some() || holder.direct_array_index().is_some();
            if !record && matches!(self.heap.get(holder), Some(Cell::Proxy { .. } | Cell::TypedArray { .. })) {
                return false;
            }
            let Some(data) = self.heap.object(holder) else {
                return false;
            };
            if data.has_exotic_access()
                || data.boxed_primitive().is_some()
                || data.has_indexed_descriptors()
                || self.shape_has_index_key(data.shape())
                || self.heap.sparse_length(holder).is_some()
                || self
                    .heap
                    .array(holder)
                    .is_some_and(|array| array.elements().iter().any(|value| !value.is_deleted()))
            {
                return false;
            }
            current = Some(data.proto());
        }
        true
    }

    pub fn set_property_attributes(
        &mut self,
        object: Value,
        key: PropertyKey,
        attributes: PropertyAttributes,
    ) {
        if let Some((shape, slot)) = self.object_property_slot(object, key) {
            let current = self.slot_attributes(self.heap.object(object).expect("shaped object"), slot);
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
            self.heap.set_object_shape(object, next_id);
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
        let Some(original) = self.heap.object(object).map(|object| object.shape()) else {
            return;
        };
        let resolved = keys
            .iter()
            .map(|&key| {
                let slot = self.property_shape_slot(original, key);
                let current = slot
                    .and_then(|slot| self.slot_attributes(self.heap.object(object)?, slot))
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
            self.heap.set_object_shape(object, shape);
        }
    }
    pub(super) fn remove_property_attributes(&mut self, object: Value, key: PropertyKey) {
        if let Some((shape, slot)) = self.object_property_slot(object, key) {
            let current = self.slot_attributes(self.heap.object(object).expect("shaped object"), slot);
            self.store_accessor_slot(object, slot, current, DEFAULT_PROPERTY_ATTRIBUTES, DEFAULT_SHAPE_ATTRIBUTES);
            let next_id = self.append_cached_shape_transition(
                shape,
                ShapeTransition::Descriptor {
                    slot: slot as u32,
                    attributes: DEFAULT_SHAPE_ATTRIBUTES,
                },
            );
            self.heap.set_object_shape(object, next_id);
            self.invalidate_method_caches_for_key(key);
            return;
        }
        if self.descriptors.remove(&(object, key)).is_some() {
            self.invalidate_field_caches();
            self.invalidate_method_caches_for_key(key);
        }
    }
    /// Keeps the slot of `object`'s property at `slot` as its new `attributes` need it: an
    /// accessor's slot holds its getter, its pair cell or `undefined` (`slot_accessor_value`),
    /// and a data property made
    /// from an accessor starts as `undefined` (ValidateAndApplyPropertyDescriptor).
    fn store_accessor_slot(
        &mut self,
        object: Value,
        slot: usize,
        current: Option<PropertyAttributes>,
        attributes: PropertyAttributes,
        recorded: ShapeAttributes,
    ) {
        let value = if recorded.slot_accessors() || attributes.accessor || current.is_some_and(|current| current.accessor) {
            self.slot_accessor_value(attributes, recorded)
        } else {
            return;
        };
        self.heap.property_set(object, slot, value);
    }
    /// Clears the field caches when a marked holder's fields moved (`Heap::take_holder_moves`): a holder
    /// cell is marked only where a field entry names it, so only such moves pay the reset.
    pub(super) fn drop_moved_holder_entries(&mut self) {
        if self.heap.take_holder_moves() {
            self.invalidate_field_caches();
        }
    }

    pub(super) fn invalidate_field_caches(&mut self) {
        self.reset_field_cache_tables();
        self.symbol_write_proofs.clear();
        self.symbol_load_proofs.clear();
        self.array_iteration_shape = None;
        self.collection_iteration_shape = None;
        self.array_species_proof = None;
        self.concat_spreadable_proof = None;
        self.for_in.forget_snapshot();
        self.species_slot_proof.set(None);
        self.promise_species_proof.take();
        self.promise_then_memo.take();
        self.heap.dependencies_mut().retire_all();
        self.enumerable_keys.clear();
        self.enumerable_key_lists.clear();
        self.enumerable_key_value_lists.clear();
        self.field_add_sets.clear();
        self.keyed_adds.clear();
        self.field_add_chains.clear();
        self.megamorphic_fields.clear();
        self.field_stubs.clear();
        self.heap.dependencies_mut().clear_inherited();
        self.accessor_fields.clear();
        self.instanceof_constructor_layouts = [None; INSTANCEOF_CONSTRUCTOR_LAYOUTS];
        self.in_presences = [None; super::in_presence::IN_PRESENCES];
        self.closure_shapes = [None; _];
        self.resolver_function_shape = None;
        self.bound_function_shape = None;
        self.function_name_shapes.clear();
        self.closure_templates.clear();
        self.iterator_result_header = None;
        self.reused_iterator_result = Value::UNDEFINED;
        self.descriptor_shapes = [None; _];
        self.descriptor_layout = None;
        self.regexp_object_header = None;
        self.regexp_last_index_layouts.clear();
        self.regexp_builtin_layouts.clear();
        self.regexp_symbol_layouts.clear();
    }
    /// Empties the per-site cache tables: the sites written since the last reset where they were
    /// tracked, else every site, which starts the tracking (`field_cache_writes`).
    fn reset_field_cache_tables(&mut self) {
        match &mut self.field_cache_writes {
            Some(sites) => {
                for &site in sites.iter() {
                    let site = site as usize;
                    self.field_caches.set(site, EMPTY_CACHE);
                    self.field_add_indices.set(site, NO_FIELD_ADD_SET);
                    self.megamorphic_field_indices.set(site, NO_MEGAMORPHIC_FIELD);
                    self.inherited_field_indices.set(site, NO_INHERITED_FIELD);
                    self.accessor_field_indices.set(site, NO_ACCESSOR_FIELD);
                }
                sites.clear();
            }
            None => {
                self.field_caches.reset();
                self.field_add_indices.reset();
                self.megamorphic_field_indices.reset();
                self.inherited_field_indices.reset();
                self.accessor_field_indices.reset();
                self.field_cache_writes = Some(Vec::new());
            }
        }
    }
    /// Notes that `site`'s per-site cache entries change (`field_cache_writes`).
    pub fn note_field_cache_write(&mut self, site: usize) {
        if let Some(sites) = &mut self.field_cache_writes {
            if sites.len() < self.field_caches.len() / FIELD_CACHE_SPARSE_RESET_SHARE {
                sites.push(site as u32);
            } else {
                self.field_cache_writes = None;
            }
        }
    }
    pub(super) fn invalidate_method_caches_for_key(&mut self, key: PropertyKey) {
        match key {
            PropertyKey::String(atom) | PropertyKey::Private(_, atom) => {
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
    pub fn current_this(&mut self, frame: usize) -> Option<Value> {
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
            .dynamic_binding(self.frame_caller(frame), atom)
            .unwrap_or(self.frames[frame].this);
        // An initialized `this` binding never changes (a second `super()` throws), so the
        // frame keeps it as its fixed receiver; only a derived constructor's binding before
        // `super()` stays to be resolved again.
        let initialized = this_is_fixed(false, value);
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
            return self.load_local_binding(p, VarOwner::Frame(frame), usize::from(local), None);
        }
        match base.register_index() {
            Some(register) => Ok(self.read(frame, register)),
            None => self.checked_this_binding(p, frame),
        }
    }
}
impl FieldCacheSet {
    fn with_pair(first: FieldCache, second: FieldCache) -> Self {
        let mut entries = [EMPTY_CACHE; crate::vm::tier_policy::POLYMORPHISM];
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
            let room = entries.len() < FIELD_MEGAMORPHIC_LIMIT;
            match entries.entry(key) {
                std::collections::hash_map::Entry::Occupied(mut slot) => {
                    slot.insert(cache);
                }
                std::collections::hash_map::Entry::Vacant(slot) if room => {
                    slot.insert(cache);
                }
                std::collections::hash_map::Entry::Vacant(_) => {}
            }
            return;
        }
        if let Some(entry) = self.entries[..usize::from(self.len)]
            .iter_mut()
            .find(|entry| entry.receiver == cache.receiver && entry.holder == cache.holder)
        {
            *entry = cache;
        } else if usize::from(self.len) < crate::vm::tier_policy::POLYMORPHISM {
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
            .map_or_else(|| usize::from(self.len), |entries| entries.len())
    }
}

#[inline(always)]
fn field_cache_key(receiver: u32, holder: u32) -> u64 {
    (u64::from(receiver) << u32::BITS) | u64::from(holder)
}
impl Vm {
    /// A fresh array of the active realm holding `length` holes: `MakeArray`'s result.
    /// Its holes are defined next (`define_literal_element`), so no young run holds them.
    pub fn hole_array(&mut self, length: usize) -> Value {
        let prototype = self.array_prototype_for_realm(self.realm.globals);
        // `new Array(n)` starts double where backings take the kind (`double_elements`): a
        // numeric program then stores and loads raw doubles. Holes read `undefined` still,
        // and a value that is no Number widens the backing once (`make_mut`).
        let elements = crate::heap::ArrayElements::double_holes(length);
        self.heap.alloc_fresh_array(prototype, elements)
    }

    /// `MakeArray`'s result (`Allocation::Array`): `hole_array`, or, for an empty literal whose
    /// binding a TypeScript annotation declares an array of Numbers (`numbers`), an array whose
    /// backing starts double (`ArrayElements::double_with_room`): its Numbers append unboxed
    /// from the first, with no int32 backing to convert later.
    pub fn make_array(&mut self, length: usize, numbers: bool) -> Value {
        if !numbers || length != 0 {
            return self.hole_array(length);
        }
        let prototype = self.array_prototype_for_realm(self.realm.globals);
        self.heap.alloc_fresh_array(prototype, crate::heap::ArrayElements::double_with_room())
    }

    /// An array literal of `length` elements, each the value `element` gives for its index:
    /// `hole_array` with each hole's definition (`define_literal_element`) made at once.
    pub fn literal_array(&mut self, length: usize, element: impl FnMut(usize) -> Value) -> Value {
        let prototype = self.array_prototype_for_realm(self.realm.globals);
        self.heap.alloc_literal_array(prototype, length, element)
    }

    #[inline(always)] pub fn object_pair(
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
    pub fn object_site_shape(&mut self, program: &ResidualProgram, site: usize) -> u32 {
        if self.specialized && program.specialized {
            let shape = self.object_shapes.get(self.object_cache_index(site));
            if shape != NO_OBJECT_SHAPE {
                return shape;
            }
        }
        self.derive_object_site_shape(program, site)
    }

    /// A nested literal of constants built from its template (`LiteralStep`). No guest code
    /// and no collection runs while it is built, so the values on the build stack need no
    /// roots; each container is made as `MakeObjectLiteral` and `MakeConstArray` make one.
    pub fn literal_template(
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
                crate::bytecode::LiteralStep::Int(value) => Value::integer(value),
                crate::bytecode::LiteralStep::Array(count) => {
                    let elements = values.split_off(values.len() - count as usize);
                    self.heap.alloc_array(crate::heap::ArrayObject::new(
                            Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                            elements,
                        ))
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
                crate::bytecode::LiteralStep::Json(constant) => {
                    let Some(crate::bytecode::Constant::String(text)) = program.constants.get(constant as usize) else {
                        return Err(JsError::validation("data literal is not a JSON text constant".into()));
                    };
                    self.json_literal(text)?
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
            self.object_shapes.set(cache_site, shape);
        }
        shape
    }
    /// The shape an empty object reaches by adding `atoms` as data properties in order: the
    /// transitions an object literal with those keys walks.
    pub(super) fn literal_shape(&mut self, atoms: &[Atom]) -> u32 {
        atoms.iter().fold(EMPTY_SHAPE, |shape, atom| {
            self.transition_shape(shape, *atom)
        })
    }
    /// A user function's own data `length`, as its closure's shape holds it: `None` for a
    /// native or bound function, whose `length` other paths answer, a shape with getter slots
    /// or an object with side descriptors (which may make it an accessor), and a deleted
    /// `length` (`Function.prototype`'s answers then).
    pub fn plain_function_length(&self, value: Value) -> Option<Value> {
        if !matches!(self.heap.function(value), Some(FunctionRecord { kind: FunctionKind::User(..), .. })) {
            return None;
        }
        let object = self.heap.object(value)?;
        if object.has_side_descriptors() || self.shapes[object.shape() as usize].facts.slot_getters() {
            return None;
        }
        let slot = self.shape_slot(object.shape(), self.length_atom)?;
        self.heap.property_get_ref(object, slot).filter(|value| !self.is_accessor_pair(object.shape(), *value))
    }

    pub fn own_property(&self, object: Value, atom: Atom) -> Option<Value> {
        if atom == self.length_atom
            && let Some(length) = self.own_array_length(object)
        {
            return Some(Value::length(length));
        }
        let object = self.heap.object(object)?;
        let slot = self.shape_slot(object.shape(), atom)?;
        self.slot_data_value(object, slot)
    }

    /// `own_property` of `atom` on `object` given its shape `slot` (`own_shape_slot`), so a walk
    /// that already resolved the slot does not resolve it again. An array's length is answered
    /// before the shape, as in `own_property`.
    pub(super) fn own_property_at(&self, object: Value, atom: Atom, slot: Option<usize>) -> Option<Value> {
        if atom == self.length_atom
            && let Some(length) = self.own_array_length(object)
        {
            return Some(Value::length(length));
        }
        // A class cell's own field reads as a record's does (R4-F): `Value::DELETED` there is absent, so the caller
        // falls through to the prototype.
        let object = self.heap.object(object)?;
        self.slot_data_value(object, slot?)
    }
    /// The target and handler of a Proxy, copied out so callers need not clone its cell.
    pub fn proxy_parts(&self, value: Value) -> Option<(Value, Value)> {
        match self.heap.get(value)? {
            Cell::Proxy {
                target, handler, ..
            } => Some((*target, *handler)),
            _ => None,
        }
    }
    #[inline] pub fn is_object_like(&self, value: Value) -> bool {
        self.heap.object(value).is_some()
            || matches!(self.heap.get(value), Some(Cell::Error(_)))
            || self.is_wasm_gc_object(value)
    }
    #[inline(always)]
    pub fn get_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Result<Value, JsError> {
        self.prepare_named_function_info_lookup(object, atom);
        // An array's own `length` answers before the cache ladder: no shape or holder entry
        // can shadow that data property, so the ladder would only find the same value.
        if let Some(length) = self.array_length_field(object, atom) {
            return Ok(length);
        }
        let probe = self.cached_field_hit(p, object, site);
        #[cfg(feature = "profile-lane-exits")]
        self.record_field_probe_path(
            site as usize,
            atom,
            match probe {
                FieldCacheProbe::Hit(_) => 0,
                FieldCacheProbe::OrdinaryMiss => 1,
                FieldCacheProbe::Ineligible => 2,
            },
        );
        match probe {
            FieldCacheProbe::Hit(value) => Ok(value),
            FieldCacheProbe::OrdinaryMiss => {
                self.get_field_polymorphic(p, object, atom, site, object.heap_reference())
            }
            FieldCacheProbe::Ineligible => {
                self.get_field_uncached(p, object, atom, site, object.heap_reference())
            }
        }
    }

    #[cfg(feature = "profile-lane-exits")]
    fn record_field_probe_path(&self, site: usize, atom: Atom, outcome: u8) {
        let Some(frame) = self.frames.last() else {
            return;
        };
        let (field_site, part) = frame.source_field.map_or((u32::MAX, 3), |(field_site, part)| {
            let part = match part {
                crate::bytecode::FieldPart::First => 0,
                crate::bytecode::FieldPart::Second => 1,
                crate::bytecode::FieldPart::Sink => 2,
            };
            (field_site as u32, part)
        });
        super::lane_exits::field_probe(
            frame.program.raw(),
            frame.function,
            frame.pc as u32,
            field_site,
            site,
            atom as u32,
            part,
            outcome,
        );
    }

    /// `array.length`: an array's own `length` is a data property no definition can turn
    /// into an accessor or hide, so reading it needs no lookup.
    #[inline(always)]
    pub fn array_length_field(&self, object: Value, atom: Atom) -> Option<Value> {
        if atom != self.length_atom {
            return None;
        }
        // The staged row answers a direct array without extras; any other (an arguments object,
        // sparse elements, a length past int32) is `own_array_length`'s.
        self.heap
            .array_record_base()
            .and_then(|records| crate::staged::elements::concrete_array_length(records, object))
            .or_else(|| self.own_array_length(object).map(Value::length))
    }

    /// The `length` of an array, from the staged row (`staged::elements::array_length`) over a
    /// direct array without extras; any other array is `own_array_length`'s.
    #[inline(always)]
    pub fn array_length_value(&self, object: Value) -> Option<Value> {
        // The row inline, the other forms out of line, so the hit is a straight line.
        if let Some(length) = self.heap.array_record_base().and_then(|records| crate::staged::elements::concrete_array_length(records, object)) {
            return Some(length);
        }
        self.array_length_general(object)
    }

    #[inline(never)]
    fn array_length_general(&self, object: Value) -> Option<Value> {
        self.own_array_length(object).map(Value::length)
    }

    /// The `length` of a direct string, from the staged row over its record (`staged::strings`);
    /// the rows' other forms (a rope, a slice, a length past int32) are read through the string.
    #[inline(always)]
    pub fn string_length_value(&self, object: Value) -> Option<Value> {
        // The row inline, the other forms out of line, so the hit is a straight line.
        if let Some(length) = self.heap.string_record_base().and_then(|records| crate::staged::string_length::concrete_string_length(records, object)) {
            return Some(length);
        }
        self.string_length_general(object)
    }

    #[inline(never)]
    fn string_length_general(&self, object: Value) -> Option<Value> {
        self.heap.string(object).map(|text| Value::length(text.len()))
    }

    /// The `length` of a direct array or string, from the one staged row over both record
    /// tables (`staged::string_length::length_of`); any other form is the general read's.
    #[inline(always)]
    pub fn length_value(&self, object: Value) -> Option<Value> {
        let (arrays, strings) = (self.heap.array_record_base()?, self.heap.string_record_base()?);
        crate::staged::string_length::concrete_length_of(arrays, strings, object)
    }

    /// `cached_field_hit` for compiled code's probe (`compiled::field_get_in_lane`), which also
    /// answers from a polymorphic site's prototype-held entries, keyed by both exact shapes.
    /// The interpreter's reads leave that tier to their miss path, which also records the
    /// site's entries: answering them here left postcss's `cloneNode` reading a stale
    /// `constructor` (JetStream postcss-wtb), whose cause remains open (Linear TOD-121).
    pub fn compiled_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> FieldCacheProbe {
        let probe = self.cached_field_hit(p, object, site);
        if !matches!(probe, FieldCacheProbe::OrdinaryMiss) {
            return probe;
        }
        let Some(receiver) = self.heap.object(object) else {
            return probe;
        };
        // The holder is an object record, as most prototypes are, or another cell, as
        // `Array.prototype` of an array receiver (`ordinary_holder_entry_value`).
        let holder = match receiver.proto_link().object_granule() {
            Some(granule) => self.heap.record_by_granule(granule),
            None => self.heap.object(receiver.proto()).and_then(|o| o.record_only()),
        };
        if let Some(holder) = holder
            && let Some(cache) = self.megamorphic_field_cache(self.field_cache_index(site), receiver.exact_shape(), holder.exact_shape())
        {
            // SAFETY: as `ordinary_holder_entry_value`: the entry was recorded for an
            // immediate-prototype data property of these exact shapes.
            return FieldCacheProbe::Hit(unsafe { self.heap.holder_property_get_flat(holder, cache.slot as usize) });
        }
        probe
    }

    /// Probe own and holder entries using the shape pinned by the receiver.
    #[inline(always)]
    pub fn cached_field_hit(
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
        if let Some(hit) = receiver.record_only().and_then(|record| self.cached_field_value(index, record)) {
            self.profile
                .field_cache_hit(usize::from(hit.tier), hit.depth);
            return FieldCacheProbe::Hit(hit.value);
        }
        // A namespace answers by its bindings and a proxy by its handler: no tier below
        // records either, so neither walks them.
        if receiver.has_exotic_access() && (receiver.is_module_namespace() || self.proxy_parts(object).is_some()) {
            return FieldCacheProbe::Ineligible;
        }
        if let Some(hit) = receiver.record_only().and_then(|record| self.cached_ordinary_holder_field_value(index, record)) {
            self.profile
                .field_cache_hit(usize::from(hit.tier), hit.depth);
            return FieldCacheProbe::Hit(hit.value);
        }
        // The tiers the lane's own handlers read past these: a receiver a heap cell holds (whose
        // ordinary holder entry `cached_ordinary_holder_field_value` has already probed), and a
        // field inherited from beyond the immediate prototype.
        if let Some(value) = self.cell_own_field_hit(p, object, site)
            .or_else(|| self.class_holder_field_hit(p, object, site))
            .or_else(|| self.inherited_field_hit(p, object, site)) {
            return FieldCacheProbe::Hit(value);
        }
        FieldCacheProbe::OrdinaryMiss
    }

    /// The own data field of a direct object that the site's primary entry names: the read
    /// `cached_field_hit` answers first, without its other tiers.
    #[inline(always)]
    pub fn primary_own_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.specialized_operations_enabled(p));
        debug_assert!(self.field_cache_index(site) < self.field_caches.len());
        let (entries, records) = (self.active_field_entries as u64, self.heap.object_record_base()? as u64);
        let offset = usize::from(site) as u64 * super::NATIVE_FIELD_CACHE_LAYOUT.entry_size as u64;
        // SAFETY: `active_field_entries` is the running program's table (`field_cache_entry`), the
        // site one of its sites, and the record table and arena the heap's own; the read ends here.
        let value = unsafe { crate::staged::concrete_site_own_read(self.heap.arena_values_base(), (entries, records), object, offset) }?;
        #[cfg(feature = "profile-field-primitives")]
        self.profile.field_primitives.hit(crate::profile::field_primitives::Path::PrimaryOwnRead);
        self.profile.field_cache_hit(0, 0);
        Some(value)
    }

    /// The two reads of a nested lookup, `base.first.second`, through the primary entries of both
    /// sites: `primary_own_field_hit` of each, the tables proved once (`staged::entry::site_chain`).
    #[inline(always)]
    pub fn primary_own_chain_hit(&mut self, p: &ResidualProgram, base: Value, (first, second): (u16, u16)) -> Option<Value> {
        debug_assert!(self.specialized_operations_enabled(p));
        let (entries, records) = (self.active_field_entries as u64, self.heap.object_record_base()? as u64);
        let offset = |site: u16| u64::from(site) * super::NATIVE_FIELD_CACHE_LAYOUT.entry_size as u64;
        // SAFETY: as `primary_own_field_hit`, for both sites.
        let value = unsafe { crate::staged::concrete_site_own_chain(self.heap.arena_values_base(), (entries, records), base, (offset(first), offset(second))) }?;
        // One hit for each of the two reads.
        self.profile.field_cache_hit(0, 0);
        self.profile.field_cache_hit(0, 0);
        Some(value)
    }

    /// The store of `value` through the site's primary entry into the own data field of a direct
    /// object, where the write barrier needs no log (`staged::entry::site_store`): the store
    /// `primary_own_field_store_hit::<false>` makes first.
    #[inline(always)]
    fn primary_site_store(&mut self, object: Value, value: Value, site: u16) -> bool {
        let Some(records) = self.heap.object_record_base() else { return false };
        let offset = usize::from(site) as u64 * super::NATIVE_FIELD_CACHE_LAYOUT.entry_size as u64;
        // SAFETY: as `primary_own_field_hit`, with the heap that owns the record table.
        unsafe { crate::staged::concrete_site_store(&self.heap, (self.active_field_entries as u64, records as u64), object, offset, value) }
    }

    /// The own data field the holderless field cache entry `cache` proves for `receiver`;
    /// `None` where its shape differs.
    #[inline(always)]
    pub(super) fn own_field_entry_hit(&self, cache: FieldCache, receiver: &Object) -> Option<Value> {
        // The exact shape matches only outside dictionary storage (`Object::exact_shape`); the
        // read is the staged `[GuardShape, LoadSlot]`, which the native tiers run from the same rows.
        // SAFETY: as for `cached_field_hit`; the exact match rules out a dictionary.
        unsafe { self.heap.own_field_hit(receiver, cache.receiver, cache.record_slot()) }
    }

    /// `own_field_entry_hit` through a lane record's own-field hint (`form_hint`), whose
    /// receiver is a layout id: one compare proves the shape and storage form.
    #[inline(always)]
    pub fn own_field_hint_hit<const FORM: u32>(&self, hint: FieldCache, receiver: &Object) -> Option<Value> {
        // SAFETY: as for `own_field_entry_hit`; the hint's shape is an own entry's.
        unsafe { self.heap.own_field_hint_hit::<FORM>(receiver, hint.receiver, hint.record_slot()) }
    }

    /// `entry`, an own entry `object` just answered, as a lane record's own-field hint: its
    /// object's layout id (`Object::hint_shape`), which the hinted hits compare
    /// whole (`own_field_hint_hit`, `hinted_primary_field_store_hit`,
    /// `hinted_polymorphic_field_hit`). An entry the object does not match stays as recorded.
    /// A class cell's entry passes through unchanged: it is class-keyed (`own_receiver_key`), and
    /// `record_object_value` names no record for a cell, so the arm below leaves it as it is. A
    /// hint is a record's layout id; a cell's hits compare its class key (`cell_own_hint`).
    #[inline(always)]
    pub fn form_hint(&self, entry: FieldCache, object: Value) -> FieldCache {
        // A hint holds a `u16` field index (`LaneInstruction::field_hint`), which a spilled slot word does not fit: a
        // spilled entry forms no hint, so its site keeps reading through the entry (`class_slot_address`).
        if is_spilled_slot(entry.slot) {
            return FieldCache::own(NO_FIELD_RECEIVER, 0);
        }
        match self.heap.record_object_value(object) {
            Some(receiver) if entry.holder == NO_FIELD_HOLDER && receiver.exact_shape() == entry.receiver =>
                FieldCache { receiver: receiver.hint_shape(), ..entry },
            _ => entry,
        }
    }

    /// The own-field hint (`FieldCache::own`) of `object`'s own data property `atom`: its exact
    /// shape and slot, which `record_field_entry_hit` reads while the shape, dictionary or not,
    /// holds. `None` for any other object, dictionary storage, a shape edited in place
    /// (`Vm::owned_shape_edits_in_place`), an accessor or an absent name; with `STORES`, also a read-only one.
    pub fn own_data_field_hint<const STORES: bool>(&self, object: Value, atom: Atom) -> Option<FieldCache> {
        let receiver = self.heap.object_record(object)?;
        // An exotic object's exact shape, or dictionary storage's, is no shape (`Object::exact_shape`).
        let shape = receiver.exact_shape();
        if shape != receiver.shape() || self.owned_shape_edits_in_place(shape) || self.atom_class(atom).contains(AtomClass::PRIVATE) {
            return None;
        }
        let slot = self.shape_slot(shape, atom)?;
        let attributes = self.slot_attributes(receiver, slot)?;
        if attributes.accessor || (STORES && !attributes.writable) {
            return None;
        }
        Some(FieldCache::own(shape, u32::try_from(slot).ok()?))
    }

    /// The field cache entry of the active program's `site`.
    #[inline(always)]
    pub fn field_cache_entry(&self, site: u16) -> FieldCache {
        debug_assert_eq!(self.active_field_entries, self.field_caches.entries_from(self.active_cache_layout().field_base));
        debug_assert!(self.field_cache_index(site) < self.field_caches.len());
        // SAFETY: `active_field_entries` is re-derived whenever the active program or the
        // table changes, and the active program's layout reserves every compiler-emitted site.
        unsafe { super::site_table::CacheSiteTable::<FieldCache>::load_from(self.active_field_entries, usize::from(site)) }
    }

    /// The primary holderless shape and slot the field cache proves for this bytecode site,
    /// which generated code reads from inline or arena storage without a VM helper.
    #[inline(always)]
    pub(super) fn native_field_load_descriptor(&self, site: u16, receiver: Option<Value>) -> Option<crate::vm::engine_types::NativeFieldLoad> {
        let cache = self.field_cache_entry(site);
        if cache.holder != NO_FIELD_HOLDER || cache.receiver == NO_FIELD_RECEIVER { return None; }
        let slot = cache.slot;
        // A shape-keyed entry never spills: its slot is an inline field index.
        if !is_class_key(cache.receiver) && is_spilled_slot(slot) { return None; }
        let shape = cache.receiver_shape();
        let storage_len = self.shapes.get(shape as usize)?.storage_len();
        if field_index_of(slot) as usize >= storage_len { return None; }
        let ty = self.heap.slot_type(shape, field_index_of(slot) as usize);
        // A class-keyed receiver is a live cell, which no record's storage describes: no plain storage.
        let storage = receiver.and_then(|receiver| self.heap.object_record(receiver))
            .filter(|object| object.exact_shape() == shape)
            .and_then(Object::plain_property_storage);
        Some(crate::vm::engine_types::NativeFieldLoad {
            shape, holder: None, slot, storage_len, storage, ty,
            inherited: None, function_shape: false, formed: None, holder_cell: None,
            class: is_class_key(cache.receiver).then_some(cache.receiver & !CLASS_KEY),
        })
    }

    /// Every holderless exact shape and slot the site's caches prove, for generated code to
    /// read by a shape switch: the primary entry, or a polymorphic site's own entries while
    /// they stay inline (`tier_policy::POLYMORPHISM`); none for a megamorphic site.
    pub fn native_field_load_descriptors(&self, site: u16) -> Vec<crate::vm::engine_types::NativeFieldLoad> {
        self.native_field_descriptors(site, false)
    }

    /// `native_field_load_descriptors` with the entries whose slot an immediate prototype
    /// holds (`NativeFieldLoad::holder`): what a read, not a store, may take from the site.
    /// The site's cached reads, with its proved inherited entries (`inherited_native_loads`)
    /// joined in: a receiver shape the cache does not name takes its proved entries' load, and
    /// receivers of one shape whose prototypes differ (sibling classes whose instances have the
    /// same fields) share one load, its constant selecting each prototype's value
    /// (`InheritedConstant::joined`).
    pub fn native_field_read_descriptors(&self, site: u16) -> Vec<crate::vm::engine_types::NativeFieldLoad> {
        let mut loads = self.native_field_descriptors(site, true);
        for load in self.inherited_native_loads(site) {
            match loads.iter_mut().find(|known| known.shape == load.shape) {
                Some(known) => if let (Some(held), Some(more)) = (known.inherited, load.inherited)
                    && let Some(joined) = held.joined(more) {
                    known.inherited = Some(joined);
                },
                None => loads.push(load),
            }
        }
        loads
    }

    /// The reads a site's proved inherited entries answer (`proved_inherited_field_hit`), as
    /// constants: a receiver of the entry's shape whose prototype link is the proved one, while
    /// the value stays (`heap::subscribe_prototype_values`) and so do the `depth` prototypes the
    /// proof read (`Heap::subscribe_prototype_chain`), reads what the proved holder held when the
    /// code was compiled, `undefined` for an absent name.
    fn inherited_native_loads(&self, site: u16) -> Vec<crate::vm::engine_types::NativeFieldLoad> {
        let mut loads = Vec::new();
        let mut index = self.inherited_field_indices.get(self.field_cache_index(site));
        while index != NO_INHERITED_FIELD {
            let cache = self.heap.dependencies().inherited(index);
            index = cache.next;
            if !cache.proved || cache.receiver == NO_FIELD_RECEIVER {
                continue;
            }
            let Some(holder) = self.heap.object_record(cache.proved_holder) else { continue };
            let (slot, value) = match cache.answer {
                InheritedFieldAnswer::Slot(slot) => match self.heap.property_get(holder, usize::from(slot)) {
                    Some(value) if !value.is_deleted() => (slot, value),
                    _ => continue,
                },
                InheritedFieldAnswer::Absent => (0, Value::UNDEFINED),
            };
            loads.push(crate::vm::engine_types::NativeFieldLoad {
                shape: cache.receiver,
                holder: Some(holder.exact_shape()),
                slot,
                storage_len: usize::from(slot) + 1,
                storage: None,
                ty: crate::typed_ir::Type::ANY,
                inherited: crate::vm::engine_types::InheritedConstant::of([(cache.proved_prototype.bits(), value, cache.depth)]), function_shape: false,
                formed: None, holder_cell: None, class: None,
            });
        }
        loads
    }

    fn native_field_descriptors(&self, site: u16, holders: bool) -> Vec<crate::vm::engine_types::NativeFieldLoad> {
        let index = self.field_cache_index(site);
        // SAFETY: compiler-produced sites index exactly-sized cache vectors.
        let table_index = unsafe { self.megamorphic_field_indices.get_unchecked(index) };
        if table_index == NO_MEGAMORPHIC_FIELD {
            let cache = self.field_cache_entry(site);
            return match cache.holder {
                NO_FIELD_HOLDER => self.native_field_load_descriptor(site, None).into_iter().collect(),
                _ if holders => self.native_field_entry(cache).into_iter().collect(),
                _ => Vec::new(),
            };
        }
        // SAFETY: every non-sentinel index was installed together with its table.
        let set = unsafe { self.megamorphic_fields.get_unchecked(table_index as usize) };
        // A site past its inline entries reads its hashed ones (`object_slots::field_load_groups`).
        let overflow = set.overflow.iter().flat_map(|entries| entries.values());
        let mut loads: Vec<_> = set.recorded().filter(|_| set.overflow.is_none()).chain(overflow)
            .filter(|entry| holders || entry.holder == NO_FIELD_HOLDER)
            .filter_map(|entry| self.native_field_entry(*entry)).collect();
        loads.sort_unstable_by_key(|load| (load.shape, load.holder));
        loads
    }

    /// The values the prototypes of exact shape `holder` hold at `slot` now, each with the
    /// link that names it: what a holder read may take while the receiver's link names one of
    /// them and no value changes (`NativeFieldLoad::inherited`). A shape does not name one prototype: sibling
    /// prototypes holding the same names share it. Each is the receiver's own prototype, the one
    /// link of its chain.
    fn inherited_constant(&self, holder: u32, slot: u16) -> Option<crate::vm::engine_types::InheritedConstant> {
        let mut holders = Vec::new();
        for prototype in self.heap.prototypes_of_shape(holder) {
            let value = self.heap.property_get_ref(self.heap.object(prototype)?, usize::from(slot))?;
            if value.is_deleted() { return None; }
            holders.push((crate::heap::ProtoLink::of(prototype).bits(), value, 1));
        }
        crate::vm::engine_types::InheritedConstant::of(holders)
    }

    /// The descriptor of one cache entry: its receiver's shape, its holder's where it names
    /// one, and the slot, which the shape that holds it must cover.
    fn native_field_entry(&self, entry: FieldCache) -> Option<crate::vm::engine_types::NativeFieldLoad> {
        if entry.receiver == NO_FIELD_RECEIVER { return None; }
        let slot = entry.slot;
        let class = is_class_key(entry.receiver).then_some(entry.receiver & !CLASS_KEY);
        // A shape-keyed entry never spills: its slot is an inline field index.
        if class.is_none() && is_spilled_slot(slot) { return None; }
        let shape = entry.receiver_shape();
        let holder = (entry.holder != NO_FIELD_HOLDER).then_some(entry.holder);
        let storage_len = self.shapes.get(holder.unwrap_or(shape) as usize)?.storage_len();
        if field_index_of(slot) as usize >= storage_len { return None; }
        let ty = self.heap.slot_type(holder.unwrap_or(shape), field_index_of(slot) as usize);
        // A class-keyed held entry names the holder cell by granule, read live: it has no inherited
        // constant. A shape-keyed one keeps the holder's layout id where it kept one, which the plan
        // checks names the holder's shape before a read guards by it.
        let (holder_cell, inherited) = match (holder, class) {
            (Some(_), Some(_)) => (Some(entry.holder_cell), None),
            (Some(holder), None) => (
                (entry.holder_cell != NO_FIELD_HOLDER_LAYOUT).then_some(entry.holder_cell),
                self.inherited_constant(holder, field_index_of(slot) as u16),
            ),
            (None, _) => (None, None),
        };
        Some(crate::vm::engine_types::NativeFieldLoad {
            shape, holder, slot, storage_len, storage: None, ty,
            inherited, function_shape: false, formed: None, holder_cell, class,
        })
    }

    /// The type `slot` of `shape` holds where a reader may take its words as tagged values, which
    /// an ahead image's guessed field read is (`aot_image::symbols::guarded_shape`): `None` where
    /// no store has typed the slot yet. A type within Number is a dependency of its reader
    /// (`SlotDependency`); any other type stays tagged.
    pub fn tagged_slot_type(&self, shape: u32, slot: u16) -> Option<crate::typed_ir::Type> {
        let ty = self.heap.slot_type(shape, usize::from(slot));
        (ty != crate::typed_ir::Type::NEVER).then_some(ty)
    }

    /// The global object's own data field a by-name load's hint (an own entry) names, its
    /// values flat or behind extras (`Heap::global_field_hit`).
    #[inline(always)]
    pub fn global_field_hint_hit(&self, hint: FieldCache) -> Option<Value> {
        let receiver = self.heap.object_record(self.realm.globals)?;
        // SAFETY: as for `own_field_entry_hit`; the hint is an own entry of the global object.
        unsafe { self.heap.global_field_hit(receiver, hint.receiver, hint.record_slot()) }
    }

    /// The value the field cache entry `cache` proves for the direct object `object`: its own
    /// data field where the entry names no holder, else that of its prototype, an object
    /// record; `None` where either shape differs. The primary own and record-holder reads
    /// make it of their site's entry; a call's read memo makes it of a copy, which holds
    /// while the shapes it names do.
    #[inline(always)]
    pub fn record_field_entry_hit(&self, cache: FieldCache, object: Value) -> Option<Value> {
        let receiver = self.heap.object_record(object)?;
        if cache.holder == NO_FIELD_HOLDER {
            let value = self.own_field_entry_hit(cache, receiver);
            #[cfg(feature = "profile-field-primitives")]
            if value.is_some() {
                self.profile.field_primitives.hit(crate::profile::field_primitives::Path::HintOwnRead);
            }
            return value;
        }
        let holder = self.heap.record_by_granule(receiver.proto_link().object_granule()?)?;
        if cache.receiver != receiver.exact_shape() || !cached_holder_matches(holder, cache.holder) {
            return None;
        }
        #[cfg(feature = "profile-field-primitives")]
        self.profile.field_primitives.hit(crate::profile::field_primitives::Path::HintHolderRead);
        // SAFETY: as `cached_ordinary_holder_field_value`.
        Some(unsafe { self.heap.holder_property_get_flat(holder, cache.slot as usize) })
    }

    /// The store `cached_field_store_hit` makes to an own data field of a direct object that
    /// the site's primary entry names, without its other tiers. Without `LOGS`, `false` also
    /// leaves a store the write barrier logs (`Heap::record_property_set_flat`).
    #[inline(always)]
    pub fn primary_own_field_store_hit<const LOGS: bool>(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
        layout: crate::heap::PropertyStoreLayout,
    ) -> bool {
        debug_assert!(self.specialized_operations_enabled(p));
        if self.method_caches_may_name(atom) {
            return false;
        }
        let Some(record_id) = object.direct_object_index().and_then(|r| self.heap.record_id(r)) else {
            // A live class cell takes its store by its class key; an object without a record, as a
            // RegExp's `lastIndex` store reaches, is out of line.
            return self.primary_class_cell_store_hit::<LOGS>(object, value, site)
                || (LOGS && self.primary_cell_field_store_hit(object, value, site, layout));
        };
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = self.field_cache_entry(site);
        let stored = (!LOGS && self.primary_site_store(object, value, site)) || self.store_record_field_entry::<LOGS>(record_id, cache, value, layout);
        if !stored {
            return false;
        }
        #[cfg(feature = "profile-field-primitives")]
        self.profile.field_primitives.hit(if LOGS {
            crate::profile::field_primitives::Path::PrimaryStoreLogged
        } else {
            crate::profile::field_primitives::Path::PrimaryStoreNoLog
        });
        self.profile.field_cache_hit(0, 0);
        true
    }

    /// Store through an own-data entry's exact shape/slot proof. Both table entries and
    /// instruction hints use this operation; the caller has excluded method-observable writes.
    #[inline(always)]
    fn store_record_field_entry<const LOGS: bool>(
        &mut self, record_id: u32, cache: FieldCache, value: Value,
        layout: crate::heap::PropertyStoreLayout,
    ) -> bool {
        if !LOGS && layout == crate::heap::PropertyStoreLayout::Preserve {
            // The unlogged store the generated store row makes through a hint: one compare
            // proves the exact shape and the arena form (`Object::formed_plain_start`), then
            // the store gate; any other form takes the staged store. A refusal changes nothing,
            // so an observed arena's store goes on to the flat store below.
            // SAFETY: a direct object's live record; the exact shape covers the slot.
            if cache.holder == NO_FIELD_HOLDER
                && unsafe { crate::staged::store::concrete_own_store(&mut self.heap, record_id, cache.receiver, false, cache.record_slot(), value) }
            {
                return true;
            }
        }
        let receiver = self.heap.object_record_by_id(record_id);
        if cache.receiver != receiver.exact_shape() || cache.holder != NO_FIELD_HOLDER {
            return false;
        }
        // The barrier's one test for this store. Without `LOGS` a logged store is left to the
        // caller; with it, a logged store takes the barriered store below.
        let logged = self.heap.object_store_logs(record_id, value);
        if logged && !LOGS {
            return false;
        }
        if !logged {
            // A plain arena run whose slot already holds the value's class: no join, no settling.
            if receiver.plain_arena_start().is_some()
                // SAFETY: the exact shape holds the slot; the record's values are a plain arena run; the barrier admitted the store.
                && unsafe { self.heap.held_arena_store(record_id, cache.receiver, cache.record_slot(), value) } {
                return true;
            }
            // SAFETY: this exact own-data shape proves the slot outside dictionary storage; the barrier admitted the store.
            return unsafe { self.heap.record_property_set_unlogged(record_id, cache.slot as usize, value, layout) };
        }
        // SAFETY: this exact own-data shape proves the slot outside dictionary storage.
        unsafe { self.heap.record_property_set_flat::<LOGS>(record_id, cache.slot as usize, value, layout) }
    }

    /// A monomorphic store's copied entry, with the no-log/preserve-layout admission of
    /// `primary_own_field_store_hit::<false>`: where the arena keeps slot representations
    /// (`TRACKED`), observed by them; otherwise with no call
    /// (`Heap::record_property_set_unobserved`), which refuses an observed store. Refusal
    /// leaves the logged path to the caller.
    #[inline(always)]
    pub fn hinted_primary_field_store_hit<const TRACKED: bool>(&mut self, hint: FieldCache, object: Value, atom: Atom, value: Value) -> bool {
        if self.method_caches_may_name(atom) {
            return false;
        }
        let Some(record_id) = object.direct_object_index().and_then(|r| self.heap.record_id(r)) else {
            return false;
        };
        if hint.holder != NO_FIELD_HOLDER {
            return false;
        }
        // SAFETY: a direct object Value names a live record; the hint's slot is one its shape covers.
        let stored = unsafe {
            if TRACKED {
                self.heap.hinted_tracked_store(record_id, hint.receiver, hint.record_slot(), value)
            } else {
                crate::staged::store::concrete_own_store(&mut self.heap, record_id, hint.receiver, true, hint.record_slot(), value)
            }
        };
        if !stored {
            return false;
        }
        #[cfg(feature = "profile-field-primitives")]
        self.profile.field_primitives.hit(crate::profile::field_primitives::Path::HintedPrimaryStore);
        self.profile.field_cache_hit(0, 0);
        true
    }

    /// `hinted_primary_field_store_hit` of a site whose objects keep narrow runs
    /// (`lane_set_field_narrow`): the value's narrow form in place (`concrete_narrow_store`).
    #[inline(always)]
    pub fn hinted_narrow_field_store_hit(&mut self, hint: FieldCache, object: Value, atom: Atom, value: Value) -> bool {
        if self.method_caches_may_name(atom) || hint.holder != NO_FIELD_HOLDER {
            return false;
        }
        let Some(record_id) = object.direct_object_index().and_then(|r| self.heap.record_id(r)) else {
            return false;
        };
        // SAFETY: a direct object Value names a live record; the hint's slot is one its shape covers.
        unsafe { crate::staged::store::concrete_narrow_store(&mut self.heap, record_id, hint.receiver, hint.record_slot(), value) }
    }

    /// The store of `value` through the site's primary entry into the field of a live class cell whose class
    /// key the entry names (`Heap::live_class_key`), in place (`Heap::class_cell_store`). Without `LOGS`, `false`
    /// also leaves a store the barrier would log, or any store while slot representations are tracked: the
    /// class cell's store takes no Number-slot conversion (`staged::slot_store::class_word`).
    #[inline(always)]
    fn primary_class_cell_store_hit<const LOGS: bool>(&mut self, object: Value, value: Value, site: u16) -> bool {
        let Some(granule) = self.heap.live_class_cell(object) else {
            return false;
        };
        let cache = self.field_cache_entry(site);
        if self.heap.live_class_key(object) != Some(cache.receiver) || cache.holder != NO_FIELD_HOLDER {
            return false;
        }
        let open = if LOGS {
            !self.heap.tracks_slot_values()
        } else {
            self.heap.store_unlogged(crate::staged::StagedRecordSpace::Class, granule, value)
        };
        if !open {
            return false;
        }
        self.heap.class_entry_store(granule, cache.slot, value);
        self.profile.field_cache_hit(0, 0);
        true
    }

    /// `primary_own_field_store_hit` of an object in a slot cell: its exact shape the site's
    /// own entry (`Heap::cell_property_set_exact`).
    #[inline(never)]
    fn primary_cell_field_store_hit(&mut self, object: Value, value: Value, site: u16, layout: crate::heap::PropertyStoreLayout) -> bool {
        let cache = self.field_cache_entry(site);
        let (shape, slot) = (cache.receiver, cache.slot as usize);
        // SAFETY: a recorded own entry names a non-dictionary shape holding `slot`.
        cache.holder == NO_FIELD_HOLDER && unsafe { self.heap.cell_property_set_exact(object, shape, slot, value, layout) }
            && {
                self.profile.field_cache_hit(0, 0);
                true
            }
    }

    /// `primary_own_field_store_hit` at a site past one shape (`field_site_polymorphic`): the
    /// own-data slot its megamorphic entries, inline or hashed, record for the receiver's
    /// exact shape, stored with the write barrier; the own entry that hit, a store record's
    /// hint (`hinted_own_field_store_hit`).
    #[inline(always)]
    pub fn polymorphic_own_field_store_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16, layout: crate::heap::PropertyStoreLayout,
    ) -> Option<FieldCache> {
        debug_assert!(self.specialized_operations_enabled(p));
        if self.method_caches_may_name(atom) {
            return None;
        }
        let record_id = self.heap.record_id(object.direct_object_index()?)?;
        let receiver = self.heap.object_record_by_id(record_id);
        let (index, shape) = (self.field_cache_index(site), receiver.exact_shape());
        let slot = self
            .inline_megamorphic_field_slot(index, shape)
            .or_else(|| self.hashed_megamorphic_own_field_slot(index, shape))?;
        // SAFETY: as for `primary_own_field_store_hit`: the entry was recorded for this exact
        // shape's own data slot.
        if !unsafe { self.heap.record_property_set_flat::<true>(record_id, usize::from(slot), value, layout) } {
            return None;
        }
        self.profile.field_cache_hit(2, 0);
        #[cfg(feature = "profile-field-primitives")]
        self.profile.field_primitives.hit(crate::profile::field_primitives::Path::PolymorphicStore);
        Some(FieldCache::own(shape, u32::from(slot)))
    }

    /// `polymorphic_own_field_store_hit` through the own entry a store record's hint names
    /// (`LaneInstruction::field_hint`), which an earlier hit of the site's entries proved for
    /// the receiver's exact shape: `false` where the shape differs or the store is refused.
    #[inline(always)]
    pub fn hinted_own_field_store_hit(&mut self, hint: FieldCache, object: Value, atom: Atom, value: Value) -> bool {
        if self.method_caches_may_name(atom) {
            return false;
        }
        let Some(record_id) = object.direct_object_index().and_then(|r| self.heap.record_id(r)) else {
            return false;
        };
        if !self.store_record_field_entry::<true>(record_id, hint, value, crate::heap::PropertyStoreLayout::MayWiden) {
            return false;
        }
        #[cfg(feature = "profile-field-primitives")]
        self.profile.field_primitives.hit(crate::profile::field_primitives::Path::HintedStore);
        self.profile.field_cache_hit(2, 0);
        true
    }

    /// The add `cached_field_add_hit` makes for a lane: an ordinary record's cached add,
    /// without the specialization test (`Vm::admits_in_lane`) or the store probe before it. The
    /// caller proved `atom` eligible (`field_add_atom_eligible`) for the site.
    #[inline(always)]
    pub fn primary_field_add_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> bool {
        debug_assert!(self.specialized_operations_enabled(p) && self.field_add_atom_eligible(atom));
        let Some(data) = self.heap.direct_object_view(object) else {
            return false;
        };
        if !data.admits_cached_add() {
            return false;
        }
        let (shape, link) = (data.shape(), data.proto_link());
        let key = self.own_receiver_key(object, shape);
        let site = self.field_cache_index(site);
        self.try_cached_field_add(object, atom, (shape, key), link, value, site)
    }

    /// `primary_field_add_hit` while the site's entry for the receiver's shape holds its
    /// chain proof (`prove_field_add_chain`) and no method cache names the key, for a receiver
    /// that is an object record: no walk, and a plain append (`Object::admits_plain_append`)
    /// the write barrier does not log makes no call, so a lane handler inlining it keeps its
    /// common path short; an add whose storage moves is made out of line
    /// (`Heap::record_property_add_moving`) and may allocate. `false` changes nothing and
    /// leaves the add to `primary_field_add_hit`.
    #[inline(always)]
    pub fn proved_field_add_hit(&mut self, object: Value, atom: Atom, value: Value, site: u16) -> bool {
        if object.direct_object_index().is_none() {
            return false;
        }
        // The receiver's view: a record, or a live class cell whose class word gives the same facts
        // (`Heap::object`). A cell's add is in place where it fits (`add_by_transition`).
        let Some(receiver) = self.heap.object(object) else {
            return false;
        };
        // The exact shape matches an entry's source, which is never a dictionary's, only
        // outside dictionary storage (`Object::exact_shape`).
        let key = self.own_receiver_key(object, receiver.exact_shape());
        let Some((_, cache)) =
            self.field_add_entry_for(self.field_cache_index(site), key, atom, receiver.proto_link())
        else {
            return false;
        };
        let transition = cache.transition();
        if !cache.proof_holds(receiver.proto_link())
            || !receiver.admits_cached_add()
            || self.method_caches_may_name(atom)
        {
            return false;
        }
        let layout = receiver.layout_id();
        if layout != transition.layouts[0] && Self::adopts_wide_run_layout(layout, transition) {
            let link = receiver.proto_link();
            self.adopt_wide_run_add_layouts(self.field_cache_index(site), key, atom, Some(link), layout);
        }
        self.add_by_transition(object, transition, value);
        self.profile.field_cache_hit(0, 0);
        true
    }

    /// The add of `value` to `object`, the direct object record `record_id` the caller admitted
    /// (`Object::admits_cached_add`), along the cached `transition`: the staged add row first
    /// (`staged::add`, which proves the before-shape and plain storage and needs no call), then,
    /// where the store gate is closed (slot representations are tracked, or the barrier would log
    /// the value), the plain append that observes the write, else the add that moves storage or
    /// allocates (`Heap::record_property_add_moving`).
    #[inline(always)]
    fn add_by_transition(&mut self, object: Value, transition: Transition, value: Value) {
        // A class-keyed transition (`Transition::is_class_keyed`) adds to the live class cell in place (R4-C4: the
        // class edge from the before-class to the after-shape); a field past the cell's region spills into its run.
        // A shape-keyed transition names a dictionary or constant cell, which has no class key: the cell's own edits take it.
        if let Some(granule) = self.heap.live_class_cell(object) {
            if transition.is_class_keyed() {
                self.heap.class_cell_add(granule, transition.record_form().before, transition.after, usize::from(transition.slot), value);
                return;
            }
            // A dictionary cell's add is its table's, a shape-keyed add too (`dictionary_cell`).
            if let Some((granule, id)) = self.heap.live_dictionary_cell(object) {
                self.heap.dictionary_cell_add(granule, id, transition.after, usize::from(transition.slot), value);
                return;
            }
            // The remaining cell is constant-marked, which has no class key: its mark clears, then the add takes the
            // class edge from its shape (`constant_cell_add`).
            let transition = transition.record_form();
            self.heap.constant_cell_add(granule, transition.before, transition.after, usize::from(transition.slot), value);
            return;
        }
        // The record path takes the transition's shapes (`record_form`), whose layouts it guards.
        let transition = transition.record_form();
        let record_id = self.heap.record_id(object.direct_object_index().expect("an add's receiver is an ordinary object")).expect("a receiver that is no class cell is a record");
        // SAFETY: `record_id` names the live record of `object`; the row proves the rest.
        if unsafe { concrete_add_property(&mut self.heap, record_id, transition, value) } {
            return;
        }
        // Where slot observation closed the gate but the after-shape's slot already holds the value's
        // type, the observation sees nothing: the same row, gated by the barrier's log alone.
        if self.heap.add_observation_holds(transition.after, usize::from(transition.slot), value)
            // SAFETY: as above.
            && unsafe { concrete_add_property_held(&mut self.heap, record_id, transition, value) }
        {
            // The class edge from the before-shape, which code holding that shape stable reads (`Heap::set_class`).
            self.heap.set_class(transition.before, transition.after);
            return;
        }
        let (slot, shape) = (usize::from(transition.slot), transition.after);
        if self.heap.object_record_by_id(record_id).admits_plain_append(slot, value)
            && self.heap.record_property_append_plain(record_id, slot, value, shape)
        {
            return;
        }
        // The add moves the run: reserve the size the shape tree predicts for it, so the
        // object's next adds append in place rather than move it again.
        let want = self.predicted_run_capacity(shape, slot + 1);
        self.heap.record_property_add_moving(object, slot, value, shape, want);
    }

    /// `define_field_cached`'s hit inline, for an object record: the site's add entry for its
    /// exact shape and `atom` proves the key absent, and a define never consults the
    /// prototype chain, so no chain proof applies; the receiver admits it as
    /// `class_field_define_target` does. A plain append makes no call, as in
    /// `proved_field_add_hit`. `false` changes nothing and leaves the define to
    /// `define_field_cached`.
    #[inline(always)]
    pub fn proved_field_define_hit(&mut self, object: Value, atom: Atom, value: Value, site: u16) -> bool {
        if object.direct_object_index().is_none() {
            return false;
        }
        let Some(receiver) = self.heap.object(object) else {
            return false;
        };
        // Boxed primitive, stack and error data live in the extras, which most receivers lack
        // (a live class cell has none).
        if !receiver.admits_cached_add() || receiver.holds_define_extras() {
            return false;
        }
        // A define of a name the receiver's shape already holds as a default data property
        // (`record_field_define_overwrite`) is CreateDataProperty's plain value write: an object
        // update's `{...state, n}` overwrites the copied `n`.
        let key = self.own_receiver_key(object, receiver.exact_shape());
        let overwrite = self.field_cache_entry(site);
        if overwrite.holder == NO_FIELD_HOLDER && overwrite.receiver == key && !self.method_caches_may_name(atom) {
            // The receiver may be a class cell, whose entry's slot may spill: `property_set` reads the field by index.
            self.heap.property_set(object, field_index_of(overwrite.slot) as usize, value);
            self.profile.field_cache_hit(0, 0);
            return true;
        }
        let Some((_, cache)) = self.field_add_entry(self.field_cache_index(site), key, atom) else {
            return false;
        };
        let transition = cache.transition();
        if self.method_caches_may_name(atom) {
            return false;
        }
        let layout = receiver.layout_id();
        if layout != transition.layouts[0] && Self::adopts_wide_run_layout(layout, transition) {
            self.adopt_wide_run_add_layouts(self.field_cache_index(site), key, atom, None, layout);
        }
        self.add_by_transition(object, transition, value);
        self.profile.field_cache_hit(0, 0);
        true
    }

    /// Whether an add entry whose `transition` a receiver of another `layout` hit moves to
    /// that receiver's layouts (`adopt_wide_run_add_layouts`): the receiver's values are in a
    /// wide arena run, as a learned receiver's are (`preshaped_receiver`), and the entry
    /// recorded other storage, as its constructor's first receiver, made before the count was
    /// learned, had. One way only, so receivers of both kinds never move the entry back and
    /// forth.
    #[inline(always)]
    fn adopts_wide_run_layout(layout: u32, transition: Transition) -> bool {
        Object::layout_is_wide_run(layout) && !Object::layout_is_wide_run(transition.layouts[0])
    }

    /// Moves the add entry of cache site index `site` for receivers of `shape` adding `atom`
    /// (for an adding store, the one whose chain proof holds for prototype `link`) to the
    /// layouts of a receiver of `layout`, where it adopts them (`adopts_wide_run_layout`). The
    /// staged add row (`add_by_transition`), which guards the layout it recorded, then takes the
    /// learned receivers' adds; any other receiver's take the plain or moving add, as every add
    /// the row leaves.
    #[cold]
    #[inline(never)]
    fn adopt_wide_run_add_layouts(&mut self, site: usize, shape: u32, atom: Atom, link: Option<ProtoLink>, layout: u32) {
        let set = self.field_add_indices.get(site);
        if set == NO_FIELD_ADD_SET {
            return;
        }
        let set = &mut self.field_add_sets[set as usize];
        let Some(index) = set.matching(shape, atom).find(|&index| link.is_none_or(|link| set.entry(index).proof_holds(link))) else {
            return;
        };
        let entry = set.entry_mut(index);
        if Self::adopts_wide_run_layout(layout, entry.transition()) {
            entry.layouts = Object::add_layouts(layout, entry.target_shape);
        }
    }

    /// The add `atom`'s site recorded that compiled code may run (`staged::add::site_add`): a define's
    /// (`chained` false) needs no chain proof, an adding store's needs the entry whose prototype chain
    /// is proved now (`FieldAddCache::proof_holds`), whose link the code then tests per receiver and
    /// whose proof it holds as a Stable fact. `None` where the site recorded none.
    pub fn native_field_add(&self, site: u16, atom: Atom, chained: bool) -> Option<AddSite> {
        let set = self.field_add_indices.get(self.field_cache_index(site));
        if set == NO_FIELD_ADD_SET {
            return None;
        }
        self.field_add_sets[set as usize].entries().filter(|cache| cache.atom == atom).find_map(|cache| {
            let link = chained.then_some(cache.proved_prototype);
            link.is_none_or(|link| cache.proof_holds(link))
                .then_some(AddSite { transition: cache.transition(), link: link.map(ProtoLink::bits), depth: cache.prototype_depth })
        })
    }

    /// Whether the site has recorded a cached property add (`try_cached_field_add`).
    pub fn field_site_adds(&self, site: u16) -> bool {
        self.field_add_indices.get(self.field_cache_index(site)) != NO_FIELD_ADD_SET
    }

    /// The data field a direct object's immediate prototype holds at the site's primary
    /// holder entry: `cached_field_hit`'s holder tier for a site whose primary entry names a
    /// prototype (`field_site_holder`).
    #[inline(always)]
    pub fn primary_holder_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.specialized_operations_enabled(p));
        let receiver = self.heap.object_record(object)?;
        let hit = self.ordinary_holder_entry_value(self.field_cache_entry(site), receiver)?;
        self.profile
            .field_cache_hit(usize::from(hit.tier), hit.depth);
        Some(hit.value)
    }

    /// `primary_holder_field_hit` when the receiver's prototype is an object record, the
    /// common holder, read without the cell families' dispatch (`Heap::object`), which is a
    /// call: `None` for any other prototype as for a miss.
    #[inline(always)]
    pub fn primary_record_holder_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.specialized_operations_enabled(p));
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
        debug_assert!(self.specialized_operations_enabled(p));
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = self.field_cache_entry(site);
        if cache.receiver != self.heap.live_class_key(object)? || cache.holder != NO_FIELD_HOLDER {
            return None;
        }
        debug_assert_eq!(cache.receiver_shape(), self.heap.object(object)?.exact_shape());
        let value = self.heap.class_entry_word(self.heap.live_class_cell(object)?, cache.slot);
        // A fixed class's absent union field is the hole: no own value, so the miss path walks the prototype.
        if value == Value::DELETED {
            return None;
        }
        self.profile.field_cache_hit(0, 0);
        Some(value)
    }

    /// The receiver key an own entry of `object`, whose exact shape is `shape`, records and matches:
    /// the live class cell's class key (`Heap::live_class_key`), else the shape itself.
    #[inline(always)]
    pub(super) fn own_receiver_key(&self, object: Value, shape: u32) -> u32 {
        self.heap.live_class_key(object).unwrap_or(shape)
    }

    /// The data field a live class cell's held entry names (`class_held_field_hit`): the read a
    /// class-keyed site answers beside `cell_own_field_hit`.
    #[inline(always)]
    pub fn class_holder_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.specialized_operations_enabled(p));
        let value = self.class_held_field_hit(self.field_cache_entry(site), object)?;
        self.profile.field_cache_hit(0, 1);
        Some(value)
    }

    /// The held field of a class-keyed entry `cache` for the live cell `object`: the receiver's class
    /// key matches (`Heap::live_class_key`), and the field is the holder cell's at the entry's slot. A
    /// shape-keyed or own entry is no held read here.
    #[inline(always)]
    fn class_held_field_hit(&self, cache: FieldCache, object: Value) -> Option<Value> {
        // A moved holder is an edge: it makes a collection due, and the collection clears the
        // caches before any later op reads (`drop_moved_holder_entries`), so no holder test here.
        if !is_class_key(cache.receiver) || cache.holder == NO_FIELD_HOLDER {
            return None;
        }
        if cache.receiver != self.heap.live_class_key(object)? {
            return None;
        }
        Some(self.heap.class_entry_word(cache.holder_cell, cache.slot))
    }

    /// `primary_holder_field_hit` for a receiver any heap cell holds, such as a function whose
    /// prototype holds the method (`f.apply`), which a direct object's record lookup misses.
    #[inline(always)]
    pub fn cell_holder_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.specialized_operations_enabled(p));
        if let Some(value) = self.class_held_field_hit(self.field_cache_entry(site), object) {
            self.profile.field_cache_hit(0, 1);
            return Some(value);
        }
        let receiver = self.heap.object(object)?;
        let hit = self.ordinary_holder_entry_value(self.field_cache_entry(site), receiver.record_only()?)?;
        self.profile
            .field_cache_hit(usize::from(hit.tier), hit.depth);
        Some(hit.value)
    }

    /// The data field a direct object inherits from beyond its immediate prototype, at the
    /// site's inherited entry (`cached_inherited_field_value`): `field_miss_data_value`'s
    /// cache hit for a site that has one (`field_site_inherited`).
    #[inline(always)]
    pub fn inherited_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        self.inherited_field_entry_hit(p, object, site).map(|(value, _)| value)
    }

    /// `inherited_field_hit` with the index of the entry that answered, which a lane record
    /// keeps as its hint (`inherited_hint_operands`).
    #[inline(always)]
    pub fn inherited_field_entry_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<(Value, u32)> {
        debug_assert!(self.specialized_operations_enabled(p));
        // The site's head entry, proved against the receiver, answers without the walk and
        // without the holder's slot read (its constant is kept under a watch on that holder):
        // a site whose reads alternate between receiver shapes walks from the head to its own.
        let site = self.field_cache_index(site);
        // SAFETY: as `cached_inherited_field_value`: every site index was installed with its entry.
        let head = unsafe { self.inherited_field_indices.get_unchecked(site) };
        if head != NO_INHERITED_FIELD && let Some(value) = self.proved_inherited_entry_value(head, object) {
            return Some((value, head));
        }
        let (receiver, kind) = self.inherited_receiver(object)?;
        let (hit, index, proof) =
            self.cached_inherited_field_value(site, receiver, kind == super::field_cache::InheritedReceiver::Function)?;
        self.inherited_entry_answers(kind, index).then_some(())?;
        if let Some(proof) = proof {
            self.prove_inherited_chain(proof);
        }
        self.profile
            .field_cache_hit(usize::from(hit.tier), hit.depth);
        Some((hit.value, index))
    }

    /// `inherited_field_hit` while the entry's proof holds (its chain's subjects unchanged and
    /// the receiver's prototype it was made under) and the holder is an object record: no
    /// chain walk and no call, so a lane handler inlining it saves no registers. `None`
    /// leaves any other read to `inherited_field_hit`.
    #[inline(always)]
    pub fn proved_inherited_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        debug_assert!(self.specialized_operations_enabled(p));
        // SAFETY: as for `cached_inherited_field_value`.
        let index = unsafe { self.inherited_field_indices.get_unchecked(self.field_cache_index(site)) };
        if index == NO_INHERITED_FIELD {
            return None;
        }
        self.proved_inherited_entry_value(index, object)
    }

    /// The operands of inherited hint `index` for a read of `atom` (`IcOp::Held`): the layout id
    /// of the receivers its proof last answered, the watch on its answer and the answer's bits;
    /// `None` where no proved entry of that atom is held, or its slot holds no constant yet.
    /// `others`: a function, array or builtin receiver reads it too, which the entry answers
    /// only where functions may stand on its chain and a builtin's own lookup is its shape's.
    #[inline(always)]
    pub fn inherited_hint_operands(&self, index: u16, atom: Atom, others: bool) -> Option<(u64, u64, u64)> {
        let cache = self.heap.dependencies().inherited_get(u32::from(index))?;
        if cache.atom != atom || !cache.proved {
            return None;
        }
        if others && !(cache.function_links && self.inherited_entry_answers(super::field_cache::InheritedReceiver::Builtin, u32::from(index))) {
            return None;
        }
        // An absent name answers `undefined` while the proof holds, which the read tests above
        // each time: its watch is one never retired.
        let (watch, value) = match cache.answer {
            InheritedFieldAnswer::Absent => (super::holder_hints::never_retired(), Value::UNDEFINED),
            InheritedFieldAnswer::Slot(_) => (std::rc::Rc::as_ptr(cache.constant_watch.as_ref()?), cache.constant),
        };
        Some((u64::from(cache.proved_layout), watch as u64, value.to_bits()))
    }

    /// The value inherited entry `index` answers for `object` while its proof holds: its
    /// constant while the holder's values hold (`constant_watch`), else the proved
    /// holder's slot, which becomes the constant.
    #[inline(always)]
    fn proved_inherited_entry_value(&mut self, index: u32, object: Value) -> Option<Value> {
        let (receiver, kind) = self.inherited_receiver(object)?;
        let function_receiver = kind == super::field_cache::InheritedReceiver::Function;
        self.inherited_entry_answers(kind, index).then_some(())?;
        let layout = receiver.layout_id();
        // SAFETY: a site's index names a recorded entry (`record_inherited_field`), and a
        // hint's was found in the cache.
        let cache = unsafe { self.heap.dependencies().inherited_unchecked(index) };
        if !cache.proved || function_receiver && !cache.function_links {
            return None;
        }
        // The layout the proof last answered proves the receiver shape and prototype link
        // at once; another layout compares them and, matching, becomes that layout.
        if layout != cache.proved_layout {
            let (shape, link) = (receiver.shape(), receiver.proto_link());
            if cache.receiver != shape || cache.proved_prototype != link {
                return None;
            }
            self.heap.dependencies_mut().inherited_mut(index).proved_layout = layout;
        }
        // SAFETY: as above.
        let cache = unsafe { self.heap.dependencies().inherited_unchecked(index) };
        let depth = u16::from(cache.depth);
        let value = match cache.answer {
            InheritedFieldAnswer::Absent => Value::UNDEFINED,
            InheritedFieldAnswer::Slot(_) if cache.constant_watch.as_ref().is_some_and(|watch| watch.is_published()) => cache.constant,
            InheritedFieldAnswer::Slot(slot) => self.hold_inherited_constant(index, (cache.proved_holder, slot))?,
        };
        self.profile.field_cache_hit(0, depth);
        Some(value)
    }

    /// The value the proved holder's `slot` holds, which becomes inherited entry `index`'s constant
    /// under a watch on the holder's prototype values.
    fn hold_inherited_constant(&mut self, index: u32, (proved_holder, slot): (Value, u16)) -> Option<Value> {
        let holder = self.heap.object_record(proved_holder)?;
        // SAFETY: the proof keeps the holder and its recorded shape.
        let value = unsafe { self.heap.property_get_shaped(holder, usize::from(slot)) }?;
        let watch = crate::heap::watch_prototype_values(holder.exact_shape());
        let cache = self.heap.dependencies_mut().inherited_mut(index);
        (cache.constant, cache.constant_watch) = (value, watch);
        Some(value)
    }

    /// Whether the site has an inherited entry (`inherited_field_hit`).
    pub(super) fn field_site_inherited(&self, site: u16) -> bool {
        self.inherited_field_indices.get(self.field_cache_index(site)) != NO_INHERITED_FIELD
    }

    /// The entry tier of a single-read site, in the order the four `field_site_*` predicates
    /// test it: an accessor, the megamorphic entries, a prototype holder, then an inherited
    /// entry, else the primary own entry. The site's cache index is derived once, so each
    /// table is read at the same index without re-deriving the active cache layout.
    pub fn field_site_tier(&self, site: u16) -> crate::vm::engine_types::FieldSiteTier {
        use crate::vm::engine_types::FieldSiteTier;
        let index = self.field_cache_index(site);
        if self.accessor_field_indices.get(index) != NO_ACCESSOR_FIELD {
            FieldSiteTier::Getter
        } else if self.megamorphic_field_indices.get(index) != NO_MEGAMORPHIC_FIELD {
            FieldSiteTier::Polymorphic
        } else if self.field_caches.get(index).holder != NO_FIELD_HOLDER {
            FieldSiteTier::Holder
        } else if self.inherited_field_indices.get(index) != NO_INHERITED_FIELD {
            FieldSiteTier::Inherited
        } else {
            FieldSiteTier::Own
        }
    }

    /// Whether a read at the site met an accessor it cached (`cached_field_getter`).
    pub(super) fn field_site_getter(&self, site: u16) -> bool {
        self.accessor_field_indices.get(self.field_cache_index(site)) != NO_ACCESSOR_FIELD
    }

    /// Whether the site's primary entry is a prototype's (`FieldSiteTier::Holder`).
    pub(super) fn field_site_holder(&self, site: u16) -> bool {
        self.field_caches.get(self.field_cache_index(site)).holder != NO_FIELD_HOLDER
    }

    /// The data field of a direct object, or of its immediate prototype held in an object
    /// record, among the site's megamorphic entries, which include the primary entry:
    /// `cached_field_hit` and its holder tier for a site past one shape.
    #[inline(always)]
    pub fn polymorphic_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        self.polymorphic_field_entries_hit::<true>(p, object, site, EMPTY_CACHE).map(|(value, _)| value)
    }

    /// The value prototype-held hint `index` (`hold_holder_hint`) holds for a read of `atom`
    /// from the direct object or array `object`: a receiver of its shape and prototype reads it
    /// again while the hint's watch holds, whose prototype, marked, has changed neither layout
    /// nor values since. `None` for any other receiver or name, a retired watch, or no such
    /// hint.
    #[inline(always)]
    pub fn holder_hint_hit(&self, index: u16, atom: Atom, object: Value) -> Option<Value> {
        self.holder_hint_hit_in(index, atom, self.hinted_receiver(object)?)
    }

    /// The operands of prototype hint `index` for a read of `atom` (`IcOp::Held`): its guard
    /// word, its watch's address and its value's bits; `None` where no hint of that atom is held.
    /// The watch lives as long as the hint, which outlives the read it is given to.
    #[inline(always)]
    pub fn holder_hint_operands(&self, index: u16, atom: Atom) -> Option<(u64, u64, u64)> {
        let hint = self.holder_hints.holder.get(usize::from(index))?;
        (hint.atom == atom).then(|| (hint.guard, std::rc::Rc::as_ptr(&hint.watch) as u64, hint.value.to_bits()))
    }

    /// `holder_hint_hit` of the object part `receiver` (`hinted_receiver`).
    #[inline(always)]
    pub(super) fn holder_hint_hit_in(&self, index: u16, atom: Atom, receiver: &crate::heap::Object) -> Option<Value> {
        let hint = self.holder_hints.holder.get(usize::from(index))?;
        // SAFETY: the hint's watch is live as long as the hint.
        (hint.atom == atom).then(|| unsafe {
            crate::staged::holder::concrete_holder_read(receiver, hint.guard, std::rc::Rc::as_ptr(&hint.watch), hint.value)
        })?
    }

    /// `holder_hint_hit` over the up to four prototype hints `arms` names, sixteen bits each,
    /// an index plus one (zero for none): a polymorphic read whose receivers' prototypes each hold
    /// the name (`polymorphic_entry`). The receiver's object part is resolved once.
    #[inline(always)]
    pub fn holder_arms_hit(&self, arms: u64, atom: Atom, object: Value) -> Option<Value> {
        if arms == 0 {
            return None;
        }
        let receiver = self.hinted_receiver(object)?;
        let mut rest = arms;
        while rest != 0 {
            let arm = rest as u16;
            if arm != 0
                && let Some(value) = self.holder_hint_hit_in(arm - 1, atom, receiver)
            {
                return Some(value);
            }
            rest >>= u16::BITS;
        }
        None
    }

    /// Whether the holder hint `index` is held and its watch still published.
    #[inline(always)]
    pub fn holder_hint_live(&self, index: u16) -> bool {
        self.holder_hints.holder.get(usize::from(index)).is_some_and(|hint| hint.watch.is_published())
    }

    /// The operands of string hint `index` for a read of `atom` (`IcOp::HeldString`): its watch's
    /// address and its value's bits; `None` where no `String.prototype` hint of that atom is held.
    #[inline(always)]
    pub fn string_hint_operands(&self, index: u16, atom: Atom) -> Option<(u64, u64)> {
        let hint = self.holder_hints.holder.get(usize::from(index))?;
        (hint.atom == atom && hint.holder == self.string_proto)
            .then(|| (std::rc::Rc::as_ptr(&hint.watch) as u64, hint.value.to_bits()))
    }

    /// The operands of number hint `index` for a read of `atom` (`IcOp::HeldNumber`): its watch's
    /// address and its value's bits; `None` where no `Number.prototype` hint of that atom is held.
    #[inline(always)]
    pub fn number_hint_operands(&self, index: u16, atom: Atom) -> Option<(u64, u64)> {
        let hint = self.holder_hints.holder.get(usize::from(index))?;
        (hint.atom == atom && hint.holder == self.number_proto)
            .then(|| (std::rc::Rc::as_ptr(&hint.watch) as u64, hint.value.to_bits()))
    }

    /// The object part of a direct object, array, function or ordinary builtin, whose own
    /// shape and prototype link a prototype-held hint tests (`holder_hint_hit`).
    #[inline(always)]
    fn hinted_receiver(&self, object: Value) -> Option<&crate::heap::Object> {
        if let Some(record_id) = object.direct_object_index().and_then(|r| self.heap.record_id(r)) { return Some(self.heap.object_record_by_id(record_id)); }
        self.heap.held_other_receiver(object)
    }

    /// Makes `value`, which the immediate prototype of the direct object or array `object`
    /// answered for a read of `atom`, a record's hint (`hold_prototype_hint`); `None` for
    /// another receiver.
    pub fn hold_holder_hint(&mut self, owned: Option<u16>, atom: Atom, object: Value, value: Value, limit: u16) -> Option<u16> {
        let receiver = self.hinted_receiver(object)?;
        // Both header words, storage form and prototype link included, which `holder_read`
        // compares.
        let (guard, prototype) = (receiver.guard_word(), receiver.proto());
        self.hold_prototype_hint(owned, atom, guard, prototype, value, limit)
    }

    /// Makes `value`, which `String.prototype` answered for a string primitive's read of
    /// `atom`, a record's hint (`hold_prototype_hint`).
    pub fn hold_string_hint(&mut self, owned: Option<u16>, atom: Atom, value: Value, limit: u16) -> Option<u16> {
        // No object's auxiliary word is `NO_FIELD_RECEIVER`, so no object matches the guard.
        self.hold_prototype_hint(owned, atom, u64::from(NO_FIELD_RECEIVER) << u32::BITS, self.string_proto, value, limit)
    }

    /// Makes `value`, which `Number.prototype` answered for a number primitive's read of `atom`, a
    /// record's hint (`hold_prototype_hint`).
    pub fn hold_number_hint(&mut self, owned: Option<u16>, atom: Atom, value: Value, limit: u16) -> Option<u16> {
        // As `hold_string_hint`: no object's auxiliary word is `NO_FIELD_RECEIVER`.
        self.hold_prototype_hint(owned, atom, u64::from(NO_FIELD_RECEIVER) << u32::BITS, self.number_proto, value, limit)
    }

    /// Makes `value`, which `prototype` answered for a read of `atom` from a receiver of
    /// exact shape `receiver`, a record's hint: in place of `owned`, the hint the record
    /// holds, else as a new one; its index, `None` once the indices reach `limit`. The
    /// prototype is marked first, so a change to its values retires the hint's watch.
    fn hold_prototype_hint(&mut self, owned: Option<u16>, atom: Atom, guard: u64, prototype: Value, value: Value, limit: u16) -> Option<u16> {
        self.heap.mark_prototype(prototype);
        let watch = crate::heap::watch_prototype_values(self.heap.object(prototype)?.exact_shape())?;
        let hint = super::holder_hints::HolderHint { guard, atom, holder: prototype, value, watch };
        if let Some(index) = owned.filter(|&index| usize::from(index) < self.holder_hints.holder.len()) {
            self.holder_hints.holder[usize::from(index)] = hint;
            return Some(index);
        }
        let index = u16::try_from(self.holder_hints.holder.len()).ok().filter(|&index| index < limit)?;
        self.holder_hints.holder.push(hint);
        Some(index)
    }

    /// `polymorphic_field_hit` with which entry answered, whose own one a lane record keeps as
    /// its hint (`LaneInstruction::field_hint`).
    #[inline(always)]
    pub(super) fn polymorphic_field_entry_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<(Value, PolymorphicHit)> {
        self.polymorphic_field_entries_hit::<true>(p, object, site, EMPTY_CACHE)
    }

    /// `polymorphic_field_hit` among the inline entries alone: `None` for a site whose
    /// entries moved to the hash, which the general cached read (`probed_field_miss_value`)
    /// answers. Its probe keeps few values live, so a lane handler inlining it saves no
    /// registers.
    #[inline(always)]
    pub fn inline_polymorphic_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
    ) -> Option<Value> {
        self.polymorphic_field_entries_hit::<false>(p, object, site, EMPTY_CACHE).map(|(value, _)| value)
    }

    /// `inline_polymorphic_field_hit` that tests `hint`, an own entry of the site a lane
    /// record keeps (`LaneInstruction::field_hint`), before the site's entries; with which
    /// entry answered.
    #[inline(always)]
    pub fn hinted_polymorphic_field_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
        hint: FieldCache,
    ) -> Option<(Value, PolymorphicHit)> {
        self.polymorphic_field_entries_hit::<false>(p, object, site, hint)
    }

    /// `polymorphic_field_hit`, probing the hashed entries where `HASHED`, after `hint`, an own
    /// entry of the site (`EMPTY_CACHE` for none); with which entry answered.
    #[inline(always)]
    fn polymorphic_field_entries_hit<const HASHED: bool>(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        site: u16,
        hint: FieldCache,
    ) -> Option<(Value, PolymorphicHit)> {
        debug_assert!(self.specialized_operations_enabled(p));
        debug_assert!(hint.holder == NO_FIELD_HOLDER);
        // Any record receiver, an array's too: entries name only shape slots, which an
        // array's length and elements are not.
        let receiver = self.heap.record_object_value(object)?;
        // Exact shapes: a match proves a layout outside dictionary storage, so each hit
        // reads its slot flat (`Object::exact_shape`).
        // The hint is a layout id (`form_hint`); the entries record exact shapes.
        if hint.receiver == receiver.hint_shape() {
            // SAFETY: as for `cached_field_hit`; the hint is one of the site's own entries.
            let value = unsafe { self.heap.property_get_flat(receiver, hint.slot as usize) };
            self.profile.field_cache_hit(2, 0);
            return Some((value, PolymorphicHit::Hint));
        }
        let shape = receiver.exact_shape();
        let index = self.field_cache_index(site);
        // SAFETY: compiler-produced sites index exactly-sized cache vectors;
        // every non-sentinel index was installed together with its table.
        let table_index = unsafe { self.megamorphic_field_indices.get_unchecked(index) };
        if table_index == NO_MEGAMORPHIC_FIELD {
            return None;
        }
        let set = unsafe { self.megamorphic_fields.get_unchecked(table_index as usize) };
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
                let value = unsafe { self.heap.property_get_flat(receiver, entry.slot as usize) };
                self.profile.field_cache_hit(2, 0);
                return Some((value, PolymorphicHit::Own(FieldCache::own(shape, entry.slot))));
            }
            let holder = self.heap.record_prototype(receiver)?;
            let entry = set.get(shape, holder.exact_shape())?;
            debug_assert!(cached_holder_matches(holder, entry.holder));
            // SAFETY: as for the inline holder entries below.
            let value = unsafe { self.heap.holder_property_get_flat(holder, entry.slot as usize) };
            self.profile.field_cache_hit(2, 1);
            return Some((value, PolymorphicHit::Holder(entry)));
        }
        for entry in set.recorded() {
            if entry.receiver != shape {
                continue;
            }
            if entry.holder == NO_FIELD_HOLDER {
                // SAFETY: as for `cached_field_hit`.
                let value = unsafe { self.heap.property_get_flat(receiver, entry.slot as usize) };
                self.profile.field_cache_hit(2, 0);
                return Some((value, PolymorphicHit::Own(FieldCache::own(shape, entry.slot))));
            }
            // A prototype outside the records takes the slow path.
            let holder = self.heap.record_prototype(receiver)?;
            if !cached_holder_matches(holder, entry.holder) {
                continue;
            }
            // SAFETY: both immutable shapes and the property slot were recorded together
            // after confirming an immediate-prototype data property.
            let value = unsafe { self.heap.holder_property_get_flat(holder, entry.slot as usize) };
            self.profile.field_cache_hit(2, 1);
            return Some((value, PolymorphicHit::Holder(*entry)));
        }
        None
    }

    /// A read after `cached_field_hit` missed, up to the point where it would run guest code:
    /// that probe tried the site's own, megamorphic and holder entries, so the data lookup of
    /// `get_field_miss` alone. `None` leaves the read to `get_property`.
    pub fn probed_field_miss_value(&mut self, object: Value, atom: Atom, site: u16) -> Option<Value> {
        self.profile.field_cache(false);
        self.field_miss_data_value(object, atom, self.field_cache_index(site))
    }

    /// Whether the site's shapes moved to megamorphic entries, inline or hashed:
    /// `polymorphic_field_hit` then sees every entry the site has recorded.
    pub fn field_site_polymorphic(&self, site: u16) -> bool {
        self.megamorphic_field_indices.get(self.field_cache_index(site)) != NO_MEGAMORPHIC_FIELD
    }

    /// Whether the site's megamorphic entries moved to the hash (`FieldCacheSet::overflow`),
    /// which only `polymorphic_field_hit` probes.
    pub fn field_site_hashed(&self, site: u16) -> bool {
        let table_index = self.megamorphic_field_indices.get(self.field_cache_index(site));
        table_index != NO_MEGAMORPHIC_FIELD
            && self.megamorphic_fields.get(table_index as usize).is_some_and(|set| set.overflow.is_some())
    }

    /// An own-data slot from a site's inline megamorphic entries; the hashed overflow is
    /// left to `cached_field_value`.
    #[inline(always)]
    fn inline_megamorphic_field_slot(&self, site: usize, receiver: u32) -> Option<u16> {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors;
        // every non-sentinel index was installed together with its table.
        let table_index = unsafe { self.megamorphic_field_indices.get_unchecked(site) };
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
            .record_slot();
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
        let table_index = unsafe { self.megamorphic_field_indices.get_unchecked(site) };
        if table_index == NO_MEGAMORPHIC_FIELD {
            return None;
        }
        let set = unsafe { self.megamorphic_fields.get_unchecked(table_index as usize) };
        set.overflow.as_ref()?;
        set.get(receiver, NO_FIELD_HOLDER).map(|entry| entry.record_slot())
    }

    /// `get_field_cached`'s miss path past `cached_field_hit`, which already probed the
    /// receiver's own entries and its ordinary holder entry: only the megamorphic
    /// prototype-holder probe remains, then the uncached path. The receiver is an ordinary object.
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
            if let Some(hit) = self.cached_holder_field_value(index, receiver, atom) {
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
        if heap_reference.is_none() && object.as_number().is_some() && self.number_proto.is_heap() {
            let lookup = if self.specialized && p.specialized {
                match self.primitive_field_cache_read(self.number_proto, atom, site) {
                    Some(Some(hit)) => FieldCacheRead::Hit(hit),
                    Some(None) => FieldCacheRead::StringMiss {
                        prototype: self.number_proto,
                        site: self.field_cache_index(site),
                    },
                    None => FieldCacheRead::Generic,
                }
            } else {
                FieldCacheRead::Generic
            };
            return self.finish_field_cache_read(p, object, atom, lookup);
        }
        let legacy_cell = match heap_reference {
            Some(HeapReference::LegacySlot(index)) => self.heap.get_legacy(index),
            // Function, iterator and collection records have no cell; their shape answers like
            // the slot forms' (`shape_property_lookup`).
            Some(HeapReference::FunctionRecord(_) | HeapReference::IteratorRecord(_) | HeapReference::CollectionRecord(_)) => {
                let lookup = if self.specialized && p.specialized {
                    self.receiver_field_cache_read(self.shape_property_lookup(object, atom).and_then(|view| view.record_only()), atom, site)
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
                | HeapReference::V128Record(_)
                | HeapReference::EnvironmentRecord(_)
                | HeapReference::SymbolRecord(_)
                | HeapReference::WasmGcRecord(_),
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
        self.primitive_field_cache_read(self.string_proto, atom, site)
    }

    /// `string_field_cache_read` for a primitive whose reads `prototype` answers.
    #[inline(always)]
    fn primitive_field_cache_read(&self, prototype: Value, atom: Atom, site: u16) -> Option<Option<FieldCacheHit>> {
        if !self.field_cache_atom_eligible(atom) {
            return None;
        }
        let receiver = self.shape_property_lookup(prototype, atom)?.record_only()?;
        Some(self.cached_field_value(self.field_cache_index(site), receiver))
    }

    /// A string primitive's `GetField` where no lookup can run guest code, in the order
    /// `get_field_cached` takes: its `length`, then the `String.prototype` data property
    /// its site cache holds (`string_prototype_field_hit`). An index, or any other key, is
    /// left to the general path.
    pub fn cached_string_field_hit(
        &self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Option<Value> {
        if atom == self.length_atom {
            return self.string_length_value(object);
        }
        self.heap.string(object)?;
        if !self.string_field_site_cacheable(p, atom) {
            return None;
        }
        // A name the chain lacks, as an absent `type`, is `undefined` by a data walk of the
        // prototype chain, which the string's own index and `length` keys never shadow. A
        // present one the site has not recorded is left to the general read, which records
        // it (`get_string_field_miss`), so the site's later reads hit.
        self.string_prototype_field_hit(site).or_else(|| {
            match self.plain_property_lookup(self.string_proto, PropertyKey::string(atom)) {
                Some(None) => Some(Value::UNDEFINED),
                _ => None,
            }
        })
    }

    /// A number, boolean, symbol or bigint primitive's `GetField` where no lookup can run guest
    /// code: the data its primitive prototype's chain holds under `atom`, or `undefined` for a
    /// name the chain lacks (an absent feature test such as `value.isColor`), as
    /// `get_property` finds it. An accessor, an exotic link and a name `primitive_read_holder`
    /// answers with a builtin where the prototype holds none are left to the general read.
    pub fn cached_primitive_field_hit(&self, object: Value, atom: Atom) -> Option<Value> {
        let prototype = self.primitive_prototype(object).filter(|_| !self.heap.is_string(object))?;
        match self.plain_property_lookup(prototype, PropertyKey::string(atom))? {
            Some(value) => Some(value),
            None if matches!(self.atom_name(atom), "toString" | "valueOf" | "toFixed" | "toPrecision") => None,
            None => Some(Value::UNDEFINED),
        }
    }

    /// Whether a string primitive's read of `atom` in `p` can take the `String.prototype`
    /// data property its site caches (`string_prototype_field_hit`): specialized code
    /// reading neither `length` nor an index, which the string itself answers, nor a name
    /// no shape answers. It depends on the site alone, so a handler checks it once.
    pub fn string_field_site_cacheable(&self, p: &ResidualProgram, atom: Atom) -> bool {
        atom != self.length_atom
            && self.specialized
            && p.specialized
            && !self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
            && self.field_cache_atom_eligible(atom)
    }

    /// The `String.prototype` data property site `site` caches, at a site that
    /// `string_field_site_cacheable` admits.
    #[inline(always)]
    pub fn string_prototype_field_hit(&self, site: u16) -> Option<Value> {
        self.prototype_field_hit(self.string_proto, site)
    }

    /// Whether a number primitive's read of `atom` in `p` can take the `Number.prototype`
    /// data property its site caches (`number_prototype_field_hit`): a number has no own
    /// properties, so any name the field cache admits.
    pub fn number_field_site_cacheable(&self, p: &ResidualProgram, atom: Atom) -> bool {
        self.specialized && p.specialized && self.number_proto.is_heap() && self.field_cache_atom_eligible(atom)
    }

    /// The `Number.prototype` data property site `site` caches.
    #[inline(always)]
    pub fn number_prototype_field_hit(&self, site: u16) -> Option<Value> {
        self.prototype_field_hit(self.number_proto, site)
    }

    /// The data property of `prototype`'s own storage that site `site` caches.
    #[inline(always)]
    fn prototype_field_hit(&self, prototype: Value, site: u16) -> Option<Value> {
        let prototype = self.heap.object_record(prototype)?;
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = self.field_cache_entry(site);
        // An entry records only a non-dictionary shape, which the exact shape matches only
        // outside dictionary storage; a polymorphic site's other entries take the walk.
        (cache.holder == NO_FIELD_HOLDER && cache.receiver == prototype.exact_shape())
            // SAFETY: the shape and its own data slot were recorded together on the miss path.
            .then(|| unsafe { self.heap.property_get_flat(prototype, cache.slot as usize) })
    }

    /// The field cache's answer for an object cell other than a string or a
    /// proxy: a hit, a miss at the resolved site, or no cacheable receiver.
    #[inline(always)]
    fn object_field_cache_read(&self, cell: &Cell, atom: Atom, site: u16) -> FieldCacheRead {
        self.receiver_field_cache_read(self.shape_property_lookup_cell(cell, self.atom_class(atom)).and_then(|view| view.record_only()), atom, site)
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
        let cache = unsafe { self.field_caches.get_unchecked(site) };
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
        self.ordinary_holder_entry_value(unsafe { self.field_caches.get_unchecked(site) }, receiver)
    }

    /// `cached_ordinary_holder_field_value` of the site entry `cache`.
    #[inline(always)]
    fn ordinary_holder_entry_value(&self, cache: FieldCache, receiver: &Object) -> Option<FieldCacheHit> {
        if cache.holder == NO_FIELD_HOLDER || cache.receiver != receiver.exact_shape() {
            return None;
        }
        // Most prototypes are object records, read from the stored link without decoding it
        // or the cell families' dispatch.
        let holder = match receiver.proto_link().object_granule() {
            Some(granule) => self.heap.object_record_by_granule(granule),
            None => self.heap.object(receiver.proto())?.record_only()?,
        };
        if !cached_holder_matches(holder, cache.holder) {
            return None;
        }
        // SAFETY: both immutable shapes and the property slot were recorded together after
        // confirming an immediate-prototype data property; the exact holder match rules out
        // a dictionary.
        let value = unsafe { self.heap.holder_property_get_flat(holder, cache.slot as usize) };
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
            Some(holder) => holder,
            None => self.non_record_hit_holder(receiver.proto(), atom)?,
        };
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = unsafe { self.field_caches.get_unchecked(site) };
        let (cache, tier) = if cache.receiver == receiver.shape() && cache.holder == holder.shape()
        {
            (cache, 0)
        } else {
            let cache = self.megamorphic_field_cache(site, receiver.shape(), holder.shape())?;
            (cache, 2)
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

    /// The holder of a field cache hit whose prototype `proto` is not a record. For an atom
    /// outside `AtomClass::FAMILY_SENSITIVE` (and not `length`), an object without exotic
    /// access answers exactly as `shape_property_lookup` does: its shape then decides the
    /// rest. The atom is fixed per site and the exotic bit is the holder's own header word,
    /// so the lookup's per-family rules are not re-run here. A dictionary holder needs no test
    /// of its own: it carries a dictionary shape id, which no recorded holder shape equals (the
    /// same argument as `cached_holder_matches`). Every other case is decided by the lookup,
    /// which is also what a cache hit compares against.
    #[inline(always)]
    fn non_record_hit_holder(&self, proto: Value, atom: Atom) -> Option<&Object> {
        if let Some(holder) = self.heap.object(proto)
            && let Some(record) = holder.record_only()
            && !holder.has_exotic_access()
            && atom != self.length_atom
            && !self.atom_class(atom).intersects(AtomClass::FAMILY_SENSITIVE)
        {
            return Some(record);
        }
        self.shape_property_lookup(proto, atom)
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
        // A record-keyed entry: a class cell's holder takes the class-keyed tiers instead.
        let Some(receiver) = self.shape_property_lookup(prototype, atom).and_then(|view| view.record_only()) else {
            return self.get_property(p, primitive, atom);
        };
        if let Some(slot) = self.shape_slot(receiver.shape(), atom)
            && let Some(value) = self.heap.property_get(receiver, slot)
        {
            if slot < FIELD_CACHE_SLOT_CAPACITY {
                self.record_field_cache(
                    site,
                    FieldCache {
                        receiver: self.own_receiver_key(prototype, receiver.shape()),
                        holder: NO_FIELD_HOLDER,
                        slot: slot as u32,
                        holder_cell: NO_FIELD_HOLDER_LAYOUT,
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
        self.drop_moved_holder_entries();
        // A getter the site's accessor entry answers is the `[[Get]]` result's whole
        // computation: its call with the receiver, without the data walk, which it would fail.
        // A trivial getter's forward is that call's value (getters-as-calls.md).
        match self.getter_entry_read(object, site) {
            super::accessor_cache::GetterEntryRead::Answer(value, _) => return Ok(value),
            super::accessor_cache::GetterEntryRead::Call(getter) => return self.call_value(p, getter, object, &[]),
            super::accessor_cache::GetterEntryRead::Miss => {}
        }
        if let Some(getter) = self.cached_getter_after_miss(object, atom, site) {
            return self.call_value(p, getter, object, &[]);
        }
        if let Some(value) = self.field_miss_data_value(object, atom, site) {
            return Ok(value);
        }
        self.record_accessor_field_cache(object, atom, site, false);
        let getter = self.cached_field_getter(object, atom, site);
        #[cfg(feature = "profile-lane-exits")]
        if getter.is_none() {
            super::lane_exits::record_detail(format!(
                "field miss {} function={} shape_answers={} dictionary={}",
                self.atom_name(atom),
                self.heap.function(object).is_some(),
                self.shape_property_lookup(object, atom).is_some(),
                self.heap.object(object).is_some_and(|data| self.shape_is_dictionary(data.shape())),
            ));
        }
        #[cfg(feature = "profile-lane-exits")]
        self.record_field_probe_path(site, atom, if getter.is_some() { 6 } else { 7 });
        match getter {
            Some(getter) => self.call_value(p, getter, object, &[]),
            None => self.get_property(p, object, atom),
        }
    }

    /// Records the accessor a field read (a store, with `stores`) at `site` meets on `object` or a prototype of it,
    /// when shapes answer every lookup from `object` to the holder. An own accessor's entry
    /// names no holder (`OWN_ACCESSOR`): a receiver of its shape holds its own accessor in
    /// that slot. Every prototype passed becomes a prototype first, and the entry keeps their
    /// chain's cell, so a later change to one retires the entry (`Vm::chain_cell`).
    pub fn record_accessor_field_cache(&mut self, object: Value, atom: Atom, site: usize, stores: bool) {
        // The shared overflow site serves reads of every atom; an entry there would answer
        // one name's read with another's getter.
        if !self.field_cache_records(site) {
            return;
        }
        // A record-keyed entry: a class cell's receiver takes the class-keyed tiers instead.
        let Some(receiver) = self.shape_property_lookup(object, atom).and_then(|view| view.record_only()) else {
            return;
        };
        let (receiver_shape, prototype, receiver_layout) = (receiver.shape(), receiver.proto(), receiver.layout_id());
        if let Some(slot) = self.shape_slot(receiver_shape, atom) {
            // An own accessor, as `debug`'s `enabled` on each logger function: every receiver
            // of the shape holds its own getter in the slot, read from it at each hit.
            let own_getter = slot < FIELD_CACHE_SLOT_CAPACITY
                && !receiver.has_side_descriptors()
                && self.slot_holds_accessor_part(receiver, slot, stores);
            if own_getter {
                let Some(cell) = self.chain_cell(prototype, 0) else {
                    return;
                };
                let slot_accessor = self.slot_accessor(receiver_shape, slot);
                self.record_accessor_fact(object, atom, receiver_layout, 0, slot, Value::UNDEFINED);
                self.record_accessor_entry(
                    site,
                    AccessorFieldCache {
                        receiver: receiver_shape,
                        layout: receiver_layout,
                        prototype,
                        holder: OWN_ACCESSOR,
                        slot: slot as u32,
                        slot_accessor,
                        cell,
                        next: NO_ACCESSOR_FIELD,
                        getter: Value::UNDEFINED,
                        answer: super::AccessorForward::None,
                    },
                );
            }
            return;
        }
        let mut link = prototype;
        let mut depth = 0u8;
        let (holder, slot) = loop {
            let Some(data) = self.heap.ordinary_view(link) else {
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
        let Some(holder_object) = self.heap.ordinary_view(holder) else {
            return;
        };
        if slot >= FIELD_CACHE_SLOT_CAPACITY
            || !self.slot_holds_accessor_part(holder_object, slot, stores)
        {
            return;
        }
        let slot_accessor = self.slot_accessor(holder_object.shape(), slot);
        // Marking a link that was no prototype yet changes no layout, so the walk's answer
        // holds under the cell made after it.
        let Some(cell) = self.chain_cell(prototype, depth) else {
            return;
        };
        self.record_accessor_fact(object, atom, receiver_layout, depth, slot, holder);
        self.record_accessor_entry(
            site,
            AccessorFieldCache {
                receiver: receiver_shape,
                layout: receiver_layout,
                prototype,
                holder,
                slot: slot as u32,
                slot_accessor,
                cell,
                next: NO_ACCESSOR_FIELD,
                getter: Value::UNDEFINED,
                answer: super::AccessorForward::None,
            },
        );
    }

    /// Records in the fact table the accessor an entry of `record_accessor_cache` proved:
    /// `hops` links above `object` (0: its own), at `slot` of `holder` (`object` itself for an
    /// own one). Only receivers proved reads start from and shapes outside dictionary mode,
    /// whose layout fixes where each name is.
    fn record_accessor_fact(&mut self, object: Value, atom: Atom, layout: u32, hops: u8, slot: usize, holder: Value) {
        use super::symbol_load_proof::{KIND_GETTER, KIND_GETTER_PAIR, KIND_GETTER_SLOT};
        let key = PropertyKey::string(atom);
        let Some(receiver) = self.proved_read_receiver(object, key) else {
            return;
        };
        let holder_object = if hops == 0 { Some(receiver) } else { self.heap.direct_object_view(holder) };
        let (Some(key32), Some(holder_object), Ok(slot32)) = (key.key32(), holder_object, u32::try_from(slot)) else {
            return;
        };
        if self.shape_is_dictionary(receiver.shape()) || self.shape_is_dictionary(holder_object.shape()) {
            return;
        }
        let kind = match self.slot_accessor(holder_object.shape(), slot) {
            super::SlotAccessor::Getter => KIND_GETTER_SLOT,
            super::SlotAccessor::Pair => KIND_GETTER_PAIR,
            super::SlotAccessor::Attributes => KIND_GETTER,
        };
        let holder_id = if hops == 0 { 0 } else { holder.direct_object_index().and_then(|r| self.heap.record_id(r)).map_or(0, |id| id + 1) };
        let prototype = receiver.proto();
        let Some(subjects) = self.heap.chain_subjects(prototype, hops) else { return };
        if let Some(entry) = self.symbol_load_proofs.record_accessor(layout, key32, hops, slot32, kind, holder_id) {
            self.hold_fact(entry, &subjects);
        }
    }

    /// The getter (`setter` false) or setter the fact table gives `key` on the record
    /// `receiver` (`record_accessor_fact`, `record_accessor` of a symbol-keyed proof): `Some`
    /// with the part, or `Some(None)` for an accessor without it, wherever the table holds an
    /// accessor answer; `None` otherwise. The one decoder of an accessor fact, for both named
    /// and symbol-keyed reads (`Vm::proved_getter`).
    #[inline(always)]
    pub(super) fn fact_accessor<'a>(&self, receiver: ObjectRef<'a>, key: PropertyKey, setter: bool) -> Option<Option<Value>> {
        use super::symbol_load_proof::{KIND_GETTER, KIND_GETTER_PAIR, KIND_GETTER_SLOT};
        let key32 = key.key32()?;
        let layout = receiver.layout_id();
        let word = self.symbol_load_proofs.word(layout, key32)?;
        let kind = word.kind();
        if !(KIND_GETTER..=KIND_GETTER_PAIR).contains(&kind) || receiver.has_side_descriptors() {
            return None;
        }
        let holder = if word.hops() == 0 {
            receiver
        } else {
            match self.symbol_load_proofs.holder(layout, key32) {
                Some(id) if id != 0 => ObjectRef::Record(self.heap.object_record_by_id(id - 1)),
                _ => self.proof_holder(receiver, word.hops())?,
            }
        };
        let slot = word.slot();
        // SAFETY: the fact names `slot` of the holder's shape: the layout fixes an own holder's,
        // and the subjects that hold the entry an inherited one's.
        let stored = || unsafe { self.heap.property_get_shaped_view(holder, slot) };
        Some(match kind {
            KIND_GETTER_SLOT => {
                if setter {
                    None
                } else {
                    Some(stored()?)
                }
            }
            KIND_GETTER_PAIR => match self.heap.get(stored()?) {
                Some(Cell::AccessorPair { setter: found, .. }) if setter => Some(*found).filter(|found| !found.is_undefined()),
                Some(Cell::AccessorPair { getter, .. }) => Some(*getter),
                _ => return None,
            },
            _ => {
                let attributes = self.slot_attributes(holder, slot)?;
                if setter { attributes.setter } else { attributes.getter }
            }
        })
    }

    /// Where `shape` keeps the getter of the accessor at `slot` (`SlotAccessor`).
    pub(super) fn slot_accessor(&self, shape: u32, slot: usize) -> super::SlotAccessor {
        match self.shape_slot_attributes(shape, slot) {
            _ if self.shape_is_dictionary(shape) => super::SlotAccessor::Attributes,
            Some(attributes) if attributes.slot_getter() => super::SlotAccessor::Getter,
            Some(attributes) if attributes.slot_accessor_pair() => super::SlotAccessor::Pair,
            _ => super::SlotAccessor::Attributes,
        }
    }

    /// Records `entry` among the site's accessor entries: one for its receiver shape is
    /// replaced, and a full site drops its oldest.
    fn record_accessor_entry(&mut self, site: usize, entry: AccessorFieldCache) {
        let head = self.accessor_field_indices.get(site);
        if let Some(slot) = record_mru_entry(&mut self.accessor_fields, head, ACCESSOR_FIELD_CACHE_WAYS, entry) {
            self.accessor_field_indices.set(site, slot);
            self.note_field_cache_write(site);
        }
    }

    /// What native accessor `getter` returns for `this` where its call is a read of the
    /// receiver's internal slots that cannot throw for it, running no guest code and
    /// allocating nothing: a typed array's geometry and buffer, or a DataView's, Map's or
    /// Set's size accessors. `None`
    /// for any other getter or receiver, whose call the general path makes.
    pub fn pure_native_getter_value(&self, getter: Value, this: Value) -> Option<Value> {
        self.pure_native_value(self.pure_native_getter(getter)?, this)
    }

    /// The native of `getter` where `pure_native_value` answers it: a function's native is
    /// fixed for its life, so a proof that names the getter may keep its native instead.
    pub(super) fn pure_native_getter(&self, getter: Value) -> Option<Native> {
        match self.heap.function(getter) {
            Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) => Some(*native),
            _ => None,
        }
    }

    /// `pure_native_getter_value` of the native getter `native`.
    pub(super) fn pure_native_value(&self, native: Native, this: Value) -> Option<Value> {
        match native {
            Native::TypedArrayLengthGetter
            | Native::TypedArrayByteLengthGetter
            | Native::TypedArrayByteOffsetGetter
            | Native::TypedArrayBufferGetter => self.typed_array_getter_value(native, this),
            Native::DataViewByteLengthGetter | Native::DataViewByteOffsetGetter => {
                let (buffer, offset, length) = self.data_view_view(this)?;
                if self.array_buffer_out_of_bounds(buffer, offset, length) {
                    return None;
                }
                let value = if native == Native::DataViewByteLengthGetter { length } else { offset };
                Some(Value::number(value as f64))
            }
            Native::MapSizeGetter => self.heap.map_entries(this).map(|entries| Value::number(entries.len() as f64)),
            Native::SetSizeGetter => self.heap.set_entries(this).map(|entries| Value::number(entries.len() as f64)),
            _ => None,
        }
    }

    /// The part of `get_field_miss` that reads a data property of a shaped receiver or its
    /// ordinary prototypes, recording the cache entry that answers it next time; it runs no
    /// guest code. `None` leaves the read to `get_property`.
    pub fn field_miss_data_value(
        &mut self,
        object: Value,
        atom: Atom,
        site: usize,
    ) -> Option<Value> {
        let receiver = self.shape_property_lookup(object, atom)?.record_only()?;
        let function_receiver = object.direct_object_index().is_none() && self.heap.function(object).is_some();
        if let Some((hit, _, proof)) = self.cached_inherited_field_value(site, receiver, function_receiver) {
            if let Some(proof) = proof {
                self.prove_inherited_chain(proof);
            }
            self.profile
                .field_cache_hit(usize::from(hit.tier), hit.depth);
            #[cfg(feature = "profile-lane-exits")]
            self.record_field_probe_path(site, atom, 3);
            return Some(hit.value);
        }
        let own = self.shape_slot(receiver.shape(), atom);
        if self.own_field_accessor(object, receiver, own, atom) {
            return None;
        }
        if let Some(slot) = own
            && let Some(value) = self.heap.property_get(receiver, slot)
        {
            #[cfg(feature = "profile-lane-exits")]
            self.record_field_probe_path(site, atom, 4);
            if slot < FIELD_CACHE_SLOT_CAPACITY {
                self.record_field_cache(
                    site,
                    FieldCache {
                        receiver: self.own_receiver_key(object, receiver.shape()),
                        holder: NO_FIELD_HOLDER,
                        slot: slot as u32,
                        holder_cell: NO_FIELD_HOLDER_LAYOUT,
                    },
                );
            }
            return Some(value);
        }
        if let Some((holder_shape, holder_cell, slot, value)) = self.immediate_prototype_data_field(object, atom)
            && slot < FIELD_CACHE_SLOT_CAPACITY
        {
            #[cfg(feature = "profile-lane-exits")]
            self.record_field_probe_path(site, atom, 4);
            let proto = self.heap.object(object).map(|object| object.proto());
            match self.heap.live_class_key(object) {
                // A live cell is keyed by its class, which fixes its prototype link. Its held entry names
                // the holder cell by granule, so the holder must be a live cell too; a record holder takes
                // no entry (a class key has no record path to read it by).
                Some(key) => {
                    let Some(proto) = proto else { return Some(value) };
                    let Some(granule) = self.heap.live_class_cell(proto) else { return Some(value) };
                    self.heap.note_holder_cell(proto);
                    self.record_field_cache(
                        site,
                        FieldCache { receiver: key, holder: holder_shape, slot: slot as u32, holder_cell: granule },
                    );
                }
                None => {
                    if let Some(proto) = proto {
                        self.heap.note_holder_cell(proto);
                    }
                    self.record_field_cache(
                        site,
                        FieldCache {
                            receiver: receiver.shape(),
                            holder: holder_shape,
                            slot: slot as u32,
                            holder_cell,
                        },
                    );
                }
            }
            return Some(value);
        }
        // A shape holds every own name of an ordinary object, every named own property of a
        // direct array (whose elements and virtual length the caller's shape lookup excludes),
        // every own name `function_links_answer` admits of a function, and every own name of
        // a collection, date, regexp, promise or iterator (`ordinary_builtin_object`).
        let shape_holds_own = self.heap.ordinary_view(object).is_some()
            || self.heap.array(object).is_some()
            || self.heap.function(object).is_some() && self.function_links_answer(atom)
            || self.heap.ordinary_builtin_object(object).is_some();
        let (cache, shapes, value) = self.inherited_field(receiver, shape_holds_own, atom)?;
        #[cfg(feature = "profile-lane-exits")]
        self.record_field_probe_path(site, atom, 5);
        if self.field_cache_records(site) {
            self.note_field_cache_write(site);
            let depth = usize::from(cache.depth);
            self.record_inherited_field(site, cache, &shapes[..depth]);
        }
        Some(value)
    }

    /// Whether `object`'s own `atom`, at `slot` of `receiver`'s shape where it has one, is an
    /// accessor, as `property_attributes` tells: a shape chain that never made one holds data
    /// properties alone, so its slot needs no attribute lookup.
    #[inline(always)]
    fn own_field_accessor(&self, object: Value, receiver: &Object, slot: Option<usize>, atom: Atom) -> bool {
        if slot.is_some() && !self.shape_may_have_accessors(receiver.shape()) {
            return false;
        }
        match slot.and_then(|slot| self.shape_slot_attributes(receiver.shape(), slot)) {
            Some(attributes) => attributes.accessor,
            None => self.side_descriptor(object, PropertyKey::string(atom)).is_some_and(|attributes| attributes.accessor),
        }
    }

    /// Whether `object`'s property at `slot` is an accessor (`slot_attributes`), without
    /// building its attributes: a shape that may hold no accessor answers at once.
    #[inline(always)]
    fn slot_holds_accessor<'a>(&self, object: impl Into<ObjectRef<'a>>, slot: usize) -> bool {
        let object: ObjectRef<'a> = object.into();
        self.shape_may_have_accessors(object.shape())
            && self.shape_slot_attributes(object.shape(), slot).is_some_and(|attributes| attributes.accessor)
    }

    /// Whether `object`'s property at `slot` is an accessor with a setter (`stores`) or a
    /// getter, as `slot_attributes` tells, building them only where its shape may hold an
    /// accessor.
    fn slot_holds_accessor_part<'a>(&self, object: impl Into<ObjectRef<'a>>, slot: usize, stores: bool) -> bool {
        let object: ObjectRef<'a> = object.into();
        self.slot_holds_accessor(object, slot)
            && self
                .slot_attributes(object, slot)
                .is_some_and(|attributes| if stores { attributes.setter } else { attributes.getter }.is_some())
    }


    /// Records `cache` as the site's newest inherited entry. An entry for its receiver shape
    /// is replaced in place; a site holding `INHERITED_FIELD_CACHE_WAYS` entries reuses its
    /// oldest one's storage, so a site's entries stay bounded however many shapes reach it.
    /// `shapes` are the entry's chain, which the cache interns (`Dependencies::intern_inherited_chain`).
    fn record_inherited_field(&mut self, site: usize, mut cache: InheritedFieldCache, shapes: &[u32]) {
        cache.chain = self.heap.dependencies_mut().intern_inherited_chain(shapes);
        let head = self.inherited_field_indices.get(site);
        if let Some(slot) =
            record_mru_entry(self.heap.dependencies_mut(), head, INHERITED_FIELD_CACHE_WAYS, cache)
        {
            self.inherited_field_indices.set(site, slot);
        }
    }

    /// An inherited data field cached by `inherited_field`: the receiver keeps its shape and
    /// each prototype up to the holder still has its recorded shape, which an entry whose chain
    /// proof the subjects of those prototypes still hold (`InheritedFieldCache::proved`) knows
    /// without the walk. Consulted on the miss path,
    /// so the inline cache tiers keep their shape. A walk's hit comes with the proof
    /// `prove_inherited_chain` records.
    #[inline(always)]
    fn cached_inherited_field_value(
        &self,
        site: usize,
        receiver: &Object,
        function_receiver: bool,
    ) -> Option<(FieldCacheHit, u32, Option<InheritedChainProof>)> {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors; every other
        // index was installed together with its entry.
        let mut index = unsafe { self.inherited_field_indices.get_unchecked(site) };
        let cache = loop {
            if index == NO_INHERITED_FIELD {
                return None;
            }
            // SAFETY: a site's index names a recorded entry (`record_inherited_field`).
            let cache = unsafe { self.heap.dependencies().inherited_unchecked(index) };
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
        if cache.proved && cache.proved_prototype == receiver.proto_link() {
            let value = match cache.answer {
                InheritedFieldAnswer::Slot(slot) => {
                    // A holder in the object records, as most prototypes are, without the
                    // out-of-line lookup of one in a slot.
                    let holder = self.heap.object(cache.proved_holder)?;
                    // SAFETY: the proof keeps the holder and its recorded shape.
                    unsafe { self.heap.property_get_shaped_view(holder, usize::from(slot)) }?
                }
                InheritedFieldAnswer::Absent => Value::UNDEFINED,
            };
            return Some((hit(value), index, None));
        }
        let mut prototype = first;
        let mut holder = None;
        let mut holder_value = Value::UNDEFINED;
        for &shape in self.heap.dependencies().inherited_chain(cache) {
            let data = self.inherited_chain_link(prototype, cache.atom, cache.function_links)?;
            if data.ordinary_shape() != shape {
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
        };
        Some((hit(value), index, Some(proof)))
    }

    /// Records that a walk proved inherited entry `proof.index`'s chain. Every link becomes
    /// a watched prototype first, and the proof is held as read from each of them
    /// (`Dependencies::prove_inherited`), so a later change to any of them retires it.
    #[cold]
    #[inline(never)]
    fn prove_inherited_chain(&mut self, proof: InheritedChainProof) {
        let Some(subjects) = self.heap.chain_subjects(proof.prototype, proof.depth) else { return };
        let dependencies = self.heap.dependencies_mut();
        let cache = dependencies.inherited_mut(proof.index);
        cache.proved_prototype = crate::heap::ProtoLink::of(proof.prototype);
        cache.proved_layout = crate::heap::NO_PROVED_LAYOUT;
        cache.proved_holder = proof.holder;
        cache.constant_watch = None;
        dependencies.prove_inherited(proof.index, &subjects);
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
        self.shape_property_lookup(value, atom)?.record_only()
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
    ) -> Option<(InheritedFieldCache, [u32; INHERITED_FIELD_CACHE_MAX_DEPTH], Value)> {
        if self.shape_slot(object.shape(), atom).is_some() {
            return None;
        }
        let function_links = self.function_links_answer(atom);
        // The chain's shapes, which `record_inherited_field` interns as the entry's `chain`.
        let mut shapes = [NO_FIELD_HOLDER; INHERITED_FIELD_CACHE_MAX_DEPTH];
        let mut cache = InheritedFieldCache {
            receiver: object.shape(),
            atom,
            chain: 0,
            depth: 0,
            function_links,
            answer: InheritedFieldAnswer::Absent,
            proved: false,
            proved_prototype: crate::heap::ProtoLink::NULL,
            proved_layout: crate::heap::NO_PROVED_LAYOUT,
            proved_holder: Value::UNDEFINED,
            constant: Value::UNDEFINED,
            constant_watch: None,
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
                return absence_cacheable.then_some((cache, shapes, Value::UNDEFINED));
            }
            let data = self.inherited_chain_link(prototype, atom, function_links)?;
            let shape = data.shape();
            if data.has_exotic_access()
                || self.shape_is_dictionary(shape)
            {
                return None;
            }
            shapes[depth] = shape;
            cache.depth = (depth + 1) as u8;
            if let Some(slot) = self.shape_slot(shape, atom) {
                // The immediate prototype is the regular field cache's holder tier.
                if depth == 0 || slot >= FIELD_CACHE_SLOT_CAPACITY || self.slot_holds_accessor(data, slot) {
                    return None;
                }
                cache.answer = InheritedFieldAnswer::Slot(slot as u16);
                return Some((cache, shapes, self.heap.property_get(data, slot)?));
            }
            prototype = data.proto();
        }
        (prototype.is_null() && absence_cacheable).then_some((cache, shapes, Value::UNDEFINED))
    }

    #[cold]
    #[inline(never)]
    /// The shape object `holder` has now and the slot of its plain data field `atom`, as a
    /// prototype read of it guards and reads them (`immediate_prototype_data_field`): `None` where
    /// `holder` is no object record, holds no such field, holds it as an accessor, is a module
    /// namespace, or keeps its properties as a dictionary. An ahead
    /// image's guessed prototype read resolves its holder guard by it (`aot_image::symbols`).
    pub fn holder_data_field(&self, holder: Value, atom: Atom) -> Option<(u32, usize)> {
        self.heap.object_record(holder)?;
        let record = self.shape_property_lookup(holder, atom)?.record_only()?;
        if record.is_module_namespace() { return None; }
        let shape = record.shape();
        // A dictionary's slots move as it changes: its shape word names no layout to read by.
        if shape & crate::value_vec::DICTIONARY_STORAGE != 0 { return None; }
        let slot = self.shape_slot(shape, atom)?;
        (!self.slot_holds_accessor(record, slot)).then_some((shape, slot))
    }

    fn immediate_prototype_data_field(
        &self,
        receiver_value: Value,
        atom: Atom,
    ) -> Option<(u32, u32, usize, Value)> {
        let holder_value = self.heap.object(receiver_value)?.proto();
        let holder = self.shape_property_lookup(holder_value, atom)?.record_only()?;
        // A module namespace's [[Get]] is exotic: no field cache names it as a holder. A
        // namespace's shape descends from its own dictionary transition
        // (`DictionaryTrigger::ModuleNamespace`), which no other object takes, so a cached
        // holder shape never matches a namespace (`cached_holder_matches`).
        if holder.is_module_namespace() {
            return None;
        }
        let holder_shape = holder.shape();
        let slot = self.shape_slot(holder_shape, atom)?;
        if self.slot_holds_accessor(holder, slot) {
            return None;
        }
        let holder_cell = holder_layout_id(holder, holder_shape);
        Some((holder_shape, holder_cell, slot, self.heap.property_get(holder, slot)?))
    }
    pub fn set_property(
        &mut self,
        object: Value,
        atom: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        self.set_shape_property(object, PropertyKey::string(atom), value)
    }

    /// `set_property`, then `set_property_attributes`: the host's way to install a property
    /// with chosen attributes. An absent data property is created with them in one shape.
    pub fn set_property_with_attributes(
        &mut self,
        object: Value,
        atom: Atom,
        value: Value,
        attributes: PropertyAttributes,
    ) -> Result<(), JsError> {
        let key = PropertyKey::string(atom);
        if let Some(data) = DataAttributes::of(attributes)
            && let Some(object_data) = self.heap.object(object)
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
        if self.is_wasm_gc_object(target) {
            return Err(self.wasm_gc_opaque_error(p));
        }
        let _stack = self.enter_stack()?;
        if self.is_private_name(atom) {
            self.check_private_brand(p, target, atom)?;
            if self.set_private_field_value(p, target, atom, value) {
                return Ok(true);
            }
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
                let own_slot = self.own_shape_slot(current, atom);
                self.property_attributes_at(current, PropertyKey::string(atom), own_slot)
                    .filter(|attributes| {
                        attributes.accessor || self.own_property_at(current, atom, own_slot).is_some()
                    })
            };
            if let Some(attributes) = held {
                found = Some((current, attributes));
                break;
            }
            let Some(data) = self.heap.object(current) else {
                break;
            };
            current = data.proto();
            if current.is_null() {
                break;
            }
        }

        if let Some((_, attributes)) = found {
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
        }

        let receiver = self.descriptor_forwarding_target(receiver);
        if self
            .heap.object(receiver)
            .is_some_and(|object| object.is_module_namespace())
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
        // A String wrapper receiver's own index is read-only (`string_wrapper`).
        if self.string_wrapper_index(receiver, PropertyKey::string(atom)).is_some()
            || (new_property && !self.heap.object(receiver).is_some_and(|object| object.is_extensible()))
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
                .heap.object(receiver)
                .is_some_and(|object| object.is_arguments_object())
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
            PropertyKey::Private(..) => {
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
                    || !self.descriptor_flag(current, self.runtime_atoms.writable)
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
        let (slot, old) = {
            let data = self
                .heap.object(object)
                .ok_or_else(|| JsError("property write on non-object".into()))?;
            let slot = self.property_shape_slot(data.shape(), key);
            let old = slot.and_then(|slot| self.heap.property_get_ref(data, slot));
            (slot, old)
        };
        self.check_property_key_write(object, key, old.is_some())?;
        match slot {
            Some(slot) => self.write_shape_slot(object, key, slot, old, value),
            None => return self.create_shape_property(object, key, value),
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
            .heap.object(object)
            .ok_or_else(|| JsError("property write on non-object".into()))?
            .shape();
        self.check_property_key_write(object, key, false)?;
        // A fixed cell's union shape already names the key, holding `Value::DELETED` as absent (R4-F): the add is
        // an in-place store into that slot, with no transition and no class change.
        if let Some(slot) = self.property_shape_slot(shape, key)
            && self.heap.live_class_cell(object).is_some()
            && attributes == DEFAULT_DATA_ATTRIBUTES
            && self.heap.object(object).is_some_and(|data| self.heap.property_get_ref(data, slot).is_none())
        {
            self.heap.property_set(object, slot, value);
            if let PropertyKey::String(atom) = key
                && self.method_caches_may_name(atom)
            {
                self.invalidate_method_caches_for_prototype_add(object, atom);
            }
            return Ok(());
        }
        let next_shape = self.transition_data_property_shape(shape, key, attributes);
        self.heap.property_add(object, value, next_shape);
        if let PropertyKey::String(atom) = key
            && self.method_caches_may_name(atom)
        {
            self.invalidate_method_caches_for_prototype_add(object, atom);
        }
        Ok(())
    }

    /// A new accessor property, reaching the shape `set_shape_property` then
    /// `set_property_attributes` would: the default data add, then the descriptor transition
    /// of its slot, without looking the just-added slot up again or writing it twice.
    /// Re-exports (`__createBinding`, `__export`) define these by the thousand.
    pub(super) fn create_shape_accessor_property(
        &mut self,
        object: Value,
        key: PropertyKey,
        attributes: PropertyAttributes,
    ) -> Result<(), JsError> {
        debug_assert!(attributes.accessor, "an accessor property has accessor attributes");
        let shape = self
            .heap.object(object)
            .ok_or_else(|| JsError("property write on non-object".into()))?
            .shape();
        self.check_property_key_write(object, key, false)?;
        let slot = u32::try_from(self.shapes[shape as usize].storage_len())
            .expect("object property index exceeds u32");
        let added = self.transition_data_property_shape(shape, key, DEFAULT_DATA_ATTRIBUTES);
        let recorded = ShapeAttributes::record(attributes, &mut self.shape_accessors);
        let next_shape = self.append_cached_shape_transition(
            added,
            ShapeTransition::Descriptor { slot, attributes: recorded },
        );
        let value = self.slot_accessor_value(attributes, recorded);
        self.heap.property_add(object, value, next_shape);
        // A getter and setter pair is held by the shape's accessor table, not by the slot, so
        // the add's store passes no barrier for it: the owner is logged as a descriptor
        // transition logs it, or a minor collection frees the young functions.
        if recorded.table_accessors() {
            self.heap.write_barrier(object);
        }
        if let PropertyKey::String(atom) = key
            && self.method_caches_may_name(atom)
        {
            self.invalidate_method_caches_for_prototype_add(object, atom);
        }
        self.invalidate_method_caches_for_key(key);
        Ok(())
    }

    #[inline(always)]
    pub fn set_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
        strict: bool,
    ) -> Result<(), JsError> {
        // A method cache naming `atom` declines the own-store probes before running them, so
        // only then is the store half still owed to the polymorphic path.
        let store_owed = self.method_caches_may_name(atom);
        if !store_owed && self.cached_own_store_hit(p, object, value, site) {
            return Ok(());
        }
        self.set_field_polymorphic(p, object, atom, value, site, strict, store_owed)
    }

    /// Stores `value` when it is an own-data store hit that no method cache can observe,
    /// which writes the cached slot in place and neither allocates nor throws. Adds,
    /// method-observable stores and misses return false for `set_field_polymorphic`.
    #[inline(always)]
    pub fn cached_field_store_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
    ) -> bool {
        !self.method_caches_may_name(atom) && self.cached_own_store_hit(p, object, value, site)
    }

    /// The own-store probes of `cached_field_store_hit` past its method-cache guard: the
    /// site's primary entry, then its megamorphic own slot, inline or hashed.
    #[inline(always)]
    fn cached_own_store_hit(&mut self, p: &ResidualProgram, object: Value, value: Value, site: u16) -> bool {
        if !(self.specialized && p.specialized) {
            return false;
        }
        let Some(data) = self.heap.object(object) else {
            return false;
        };
        if data.has_exotic_access() {
            return false;
        }
        let index = self.field_cache_index(site);
        let shape = data.shape();
        // SAFETY: the active program's layout reserves every compiler-emitted site.
        let cache = unsafe { self.field_caches.get_unchecked(index) };
        let slot = if cache.receiver == shape && cache.holder == NO_FIELD_HOLDER {
            Some(cache.record_slot())
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
        #[cfg(feature = "profile-field-primitives")]
        self.profile.field_primitives.hit(crate::profile::field_primitives::Path::CachedStore);
        self.profile.field_cache_hit(0, 0);
        true
    }

    #[quench_macros::test_visible]
    /// The raw address for a warmed, own-data store site whose exact receiver shape still
    /// matches. Shared native execution admits `writes` once, without allocation,
    /// collection, or guest calls; misses stay on the ordinary path before the store.
    ///
    /// # Safety
    /// The caller must keep `object` rooted and must use the returned pointer only during the
    /// current no-allocation native operation, without reentry or type re-admission.
    /// Every stored value must fit `writes`; references require the collector's write barrier.
    #[inline(always)]
    pub(super) unsafe fn cached_field_store_pointer(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
        writes: crate::typed_ir::Type,
    ) -> Option<*mut Value> {
        let slot = self.cached_own_writable_slot(p, object, atom, site)?;
        // SAFETY: the shared admission proves the own data slot; the caller
        // owns the raw writer's type and storage lifetime obligations.
        unsafe { self.heap.flat_property_mut_ptr_unchecked(object, slot, writes) }
    }

    /// Whether a loop region's native stores of `writes` to the own data slot `site` caches on
    /// `object` are admitted, and the slot's address for its entry record.
    pub fn cached_field_store_admission(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        site: u16,
        writes: crate::typed_ir::Type,
    ) -> Option<*mut Value> {
        let slot = self.cached_own_writable_slot(p, object, atom, site)?;
        let shape = self.heap.object(object)?.shape();
        // A Number store's own row admits the domain of the value it stores
        // (`Lowering::admit_slot_writes`): the entry keeps a Number slot's narrower type
        // rather than widening it to every Number, which would make an int32 slot decline the
        // code its compile planned on that slot.
        let current = self.heap.slot_type(shape, slot);
        let writes = if writes == crate::typed_ir::Type::NUMBER && current.is_subtype(writes) { current } else { writes };
        // SAFETY: the address goes to the entry record alone; the region writes it as its admission says.
        unsafe { self.cached_field_store_pointer(p, object, atom, site, writes) }
    }

    /// Admit an own data slot once for a no-reentry operation. Its physical
    /// encoding is independent of the shape/descriptor permission to write it.
    fn cached_own_writable_slot(&self, p: &ResidualProgram, object: Value, atom: Atom, site: u16) -> Option<usize> {
        if !(self.specialized && p.specialized)
            || !self.field_cache_atom_eligible(atom)
            || self.method_caches_may_name(atom)
        {
            return None;
        }
        let data = self.heap.object(object)?.record_only()?;
        let properties = data.properties();
        // A prototype's or a read global's stores move what code holds (`Object::note_value_write`).
        if properties.is_dictionary() || data.observes_value_writes() {
            return None;
        }
        let cache = self.field_cache_entry(site);
        if cache.holder != NO_FIELD_HOLDER || cache.receiver != data.exact_shape() {
            return None;
        }
        let slot = cache.slot as usize;
        // Integrity changes can update descriptor attributes without changing the
        // receiver's shape. Recheck the current own descriptor before bypassing the
        // ordinary SetField path, which must observe read-only and accessor semantics.
        if self.property_shape_slot(data.shape(), PropertyKey::string(atom)) != Some(slot)
            || !self
                .property_attributes(object, PropertyKey::string(atom))
                .is_some_and(|attributes| attributes.writable && !attributes.accessor)
        {
            return None;
        }
        Some(slot)
    }

    /// A cached own-data store or a cached property add to an ordinary object: neither runs
    /// guest code nor throws, so the dispatch loop's fast match completes both.
    #[inline(always)]
    pub fn cached_field_write_hit(
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
        let Some(data) = self.heap.ordinary_view(object) else {
            return false;
        };
        if !data.admits_cached_add() {
            return false;
        }
        let (shape, link) = (data.shape(), data.proto_link());
        let key = self.own_receiver_key(object, shape);
        let site = self.field_cache_index(site);
        self.try_cached_field_add(object, atom, (shape, key), link, value, site)
    }

    /// `set_field_cached` past the inline own-data store hits. `store_owed` is false when those
    /// own-store probes already missed, so only the add half of the cache can still hit.
    #[inline(never)]
    fn set_field_polymorphic(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
        strict: bool,
        store_owed: bool,
    ) -> Result<(), JsError> {
        // Only an ordinary object reaches the shape cache: a nullish base falls through to the uncached path's TypeError.
        if self.set_field_cache_hit(p, object, atom, value, site, store_owed) {
            return Ok(());
        }
        if self.is_typed_array_element(object, atom) { return self.set_property_with_program_mode(p, object, atom, value, strict); }
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
        if self.is_wasm_gc_object(object) {
            return Err(self.wasm_gc_opaque_error(p));
        }
        // Assignment to a nullish base is an abrupt completion regardless of
        // strictness. Keep it on the canonical realm-owned TypeError path;
        // the generic object mutator's fallback error is not an ECMAScript
        // error object and loses constructor identity at the Test262 boundary.
        if object.is_null() || object.is_undefined() {
            let key = self.atom_name(atom).to_owned();
            return Err(self.nullish_assignment_error(p, object, &key));
        }
        if self.is_private_name(atom) && self.set_private_field_value(p, object, atom, value) {
            return Ok(());
        }
        if let Some((target, handler)) = self.proxy_parts(object) {
            if self.is_private_name(atom) {
                if self.own_property(object, atom).is_none() && self.host.nonextensible_applies_to_private() {
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
            let written = self.proxy_set_mode(
                p,
                target,
                handler,
                object,
                PropertyKey::string(atom),
                value,
                strict,
            )?;
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
                self.call_stored_setter(p, site, setter, object, value)?;
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
                .heap.object(object)
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
                .heap.object(object)
                .is_some_and(|object| object.is_arguments_object())
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
            .heap.object(object)
            .and_then(|data| self.shape_slot(data.shape(), atom));
        self.check_property_write(object, atom, existing.is_some())?;
        let (shape, layout) = self
            .heap.object(object)
            .map_or((u32::MAX, 0), |data| (data.shape(), data.layout_id()));
        // The add entry's source, taken before the write: a live class cell's class key (`own_receiver_key`).
        let key = self.own_receiver_key(object, shape);
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
        let data = self.heap.object(object).unwrap();
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
                let guarded = self.guarded_field_add(&prototype_guard);
                self.record_field_add_cache(
                    site,
                    FieldAddCache {
                        atom,
                        source_shape: key,
                        target_shape: data_shape,
                        slot: slot as u32,
                        layouts: Object::add_layouts(layout, data_shape),
                        ..guarded
                    },
                );
            }
            self.record_field_cache(
                site,
                FieldCache {
                    receiver: data_shape,
                    holder: NO_FIELD_HOLDER,
                    slot: slot as u32,
                    holder_cell: NO_FIELD_HOLDER_LAYOUT,
                },
            );
        }
        Ok(())
    }

    /// An empty add entry proving `guard`'s prototype chain, interned (`field_add_chains`).
    fn guarded_field_add(&mut self, guard: &AddPrototypeGuard) -> FieldAddCache {
        FieldAddCache {
            prototype_depth: guard.depth,
            chain: self.field_add_chains.intern(guard.shapes()),
            ..FieldAddCache::EMPTY
        }
    }

    pub(super) fn record_field_add_cache(&mut self, site: usize, cache: FieldAddCache) {
        let cache = self.spill_encoded_add(cache);
        if !self.field_cache_records(site) {
            return;
        }
        let index = self.field_add_indices.get(site);
        if index == NO_FIELD_ADD_SET {
            self.note_field_cache_write(site);
            self.field_add_indices.set(site, self.field_add_sets.len() as u32);
            self.field_add_sets.push(FieldAddCacheSet::EMPTY);
        }
        let index = self.field_add_indices.get(site) as usize;
        self.field_add_sets[index].insert(cache);
    }

    #[inline(always)]
    fn try_cached_field_add(
        &mut self,
        object: Value,
        atom: Atom,
        (shape, key): (u32, u32),
        link: ProtoLink,
        value: Value,
        site: usize,
    ) -> bool {
        // The caller proved `object` an extensible ordinary object of `shape` whose prototype
        // `link` names.
        let Some((set, cache)) = self.field_add_entry_for(site, key, atom, link) else {
            return false;
        };
        let (target_shape, slot, transition) = (cache.target_shape, cache.slot, cache.transition());
        if !cache.proof_holds(link) && !self.prove_field_add_chain(set, key, atom, link) {
            return false;
        }
        // An object record outside dictionary storage, whose exact shape is the entry's source
        // (`proved_field_add_hit`), takes the add as the proved hit does (`add_by_transition`):
        // the staged row, the plain or reserved append, else the moving add
        // `apply_cached_field_add` makes.
        let in_entry_shape = object.direct_object_index().is_some()
            && self.heap.object(object).is_some_and(|receiver| self.own_receiver_key(object, receiver.exact_shape()) == key);
        if !in_entry_shape {
            self.apply_cached_field_add(object, atom, shape, target_shape, slot, value);
            return true;
        }
        debug_assert_eq!(field_index_of(slot) as usize, self.shapes[shape as usize].storage_len());
        self.add_by_transition(object, transition, value);
        if self.method_caches_may_name(atom) {
            self.invalidate_method_caches_for_prototype_add(object, atom);
        }
        self.profile.field_cache_hit(0, 0);
        true
    }

    /// The add entry of cache site index `site` for receivers of `shape` adding `atom`, with
    /// the index of the site's set.
    #[inline(always)]
    pub(super) fn field_add_entry(&self, site: usize, shape: u32, atom: Atom) -> Option<(usize, &FieldAddCache)> {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors; every non-sentinel
        // index was installed together with its set.
        let set = unsafe { self.field_add_indices.get_unchecked(site) };
        if set == NO_FIELD_ADD_SET {
            return None;
        }
        let set = set as usize;
        Some((set, unsafe { self.field_add_sets.get_unchecked(set) }.find(shape, atom)?))
    }

    /// `field_add_entry` for a receiver whose prototype `link` names: the entry whose chain
    /// proof holds for it where the site keeps one (`FieldAddCacheSet::find_for`).
    #[inline(always)]
    fn field_add_entry_for(&self, site: usize, shape: u32, atom: Atom, link: ProtoLink) -> Option<(usize, &FieldAddCache)> {
        // SAFETY: as `field_add_entry`'s.
        let set = unsafe { self.field_add_indices.get_unchecked(site) };
        if set == NO_FIELD_ADD_SET {
            return None;
        }
        let set = set as usize;
        Some((set, unsafe { self.field_add_sets.get_unchecked(set) }.find_for(shape, atom, link)?))
    }

    /// Adds `atom` to `object` of `shape` at `slot` by a cached transition to `target_shape`.
    #[inline(always)]
    fn apply_cached_field_add(
        &mut self,
        object: Value,
        atom: Atom,
        shape: u32,
        target_shape: u32,
        slot: u32,
        value: Value,
    ) {
        debug_assert_eq!(field_index_of(slot) as usize, self.shapes[shape as usize].storage_len());
        self.heap.record_property_add_at(object, field_index_of(slot) as usize, value, target_shape);
        if self.method_caches_may_name(atom) {
            self.invalidate_method_caches_for_prototype_add(object, atom);
        }
        self.profile.field_cache_hit(0, 0);
    }

    /// CreateDataPropertyOrThrow of the non-index key `atom` (a class field or literal
    /// property) through the site's add cache. A define never consults the prototype chain,
    /// so an entry needs only the source shape: the object's shape proves the key absent, and
    /// `class_field_define_target` proves no exotic own property or integrity level intervenes.
    pub fn define_field_cached(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
        function: u32,
    ) -> Result<(), JsError> {
        let key = PropertyKey::string(atom);
        let cacheable = self.specialized && p.specialized && self.field_cache_atom_eligible(atom);
        let Some(shape) = self.class_field_define_target(object).filter(|_| cacheable) else {
            return self.define_class_field(p, object, key, value);
        };
        // The site's proved define, as the lane takes it inline: above all the plain value write
        // over a name the receiver's exact shape already holds, as a receiver made in its
        // constant layout does (`preshaped_receiver`).
        if self.proved_field_define_hit(object, atom, value, site) {
            return Ok(());
        }
        let site = self.field_cache_index(site);
        let receiver_key = self.own_receiver_key(object, shape);
        if let Some((_, cache)) = self.field_add_entry(site, receiver_key, atom) {
            let (target_shape, slot) = (cache.target_shape, cache.slot);
            debug_assert!(self.class_field_is_plain_add(object, key));
            self.apply_cached_field_add(object, atom, shape, target_shape, slot, value);
            return Ok(());
        }
        if let Some(entry) = self.field_define_overwrite(object, atom) {
            self.define_class_field(p, object, key, value)?;
            self.record_field_cache(site, entry);
            return Ok(());
        }
        let plain = self.class_field_is_plain_add(object, key);
        let layout = self.heap.object(object).map_or(0, |object| object.layout_id());
        self.define_class_field(p, object, key, value)?;
        let target_shape = self.heap.object(object).map_or(shape, |object| object.shape());
        if plain
            && !self.shape_is_dictionary(shape)
            && !self.shape_is_dictionary(target_shape)
            && let Some(slot) = self.shape_slot(target_shape, atom)
            && slot == self.shapes[shape as usize].storage_len()
            && let Ok(slot) = u32::try_from(slot)
        {
            // Once per transition: the add that creates `target_shape` seeds its slot from the
            // class's declared type, once the added `value` satisfies it; later adds of the shape
            // reuse this entry and its seed.
            if let Some(ty) = declared_field_type(p, function, atom) {
                self.heap.seed_declared_slot(target_shape, slot as usize, ty, value);
            }
            self.record_field_add_cache(
                site,
                FieldAddCache {
                    atom,
                    source_shape: shape,
                    target_shape,
                    slot,
                    layouts: Object::add_layouts(layout, target_shape),
                    ..FieldAddCache::EMPTY
                },
            );
        }
        Ok(())
    }

    /// The own entry a define of `atom` on `object` may take next time as a plain value write
    /// (`proved_field_define_hit`): its exact shape holds `atom` as a data property that is
    /// writable, enumerable and configurable, which CreateDataProperty leaves as it is but its
    /// value. A shape names its slots' attributes, so the exact shape proves them again.
    /// `None` for any other property, a dictionary or an object with side descriptors.
    fn field_define_overwrite(&self, object: Value, atom: Atom) -> Option<FieldCache> {
        // record-only: side descriptors and the slot attributes of the record
        let data = self.heap.object(object)?.record_only()?;
        if data.has_side_descriptors() || self.shape_is_dictionary(data.shape()) {
            return None;
        }
        let slot = self.shape_slot(data.shape(), atom)?;
        let attributes = self.slot_attributes(data, slot)?;
        (!attributes.accessor && attributes.writable && attributes.enumerable && attributes.configurable)
            .then(|| Some(FieldCache::own(data.exact_shape(), u16::try_from(slot).ok()?))).flatten()
    }

    /// The prototype shapes a cached add of `key` to `object` must find again, as the depth and
    /// shape fields of a `FieldAddCache`; `None` when a link is exotic, a dictionary, or holds
    /// `key` as an accessor or read-only.
    pub(super) fn add_prototype_guard(&self, object: Value, key: PropertyKey) -> Option<AddPrototypeGuard> {
        let mut prototype = self.heap.object(object)?.proto();
        let mut guard = AddPrototypeGuard::EMPTY;
        while !prototype.is_null() {
            let depth = usize::from(guard.depth);
            if depth == FIELD_ADD_CACHE_MAX_PROTO_DEPTH {
                return None;
            }
            let data = self.heap.object_record(prototype)?;
            let shape = data.shape();
            if data.has_exotic_access()
                || self.shape_is_dictionary(shape)
                || self
                    .property_attributes(prototype, key)
                    .is_some_and(|attributes| attributes.accessor || !attributes.writable)
            {
                return None;
            }
            guard.shapes[depth] = shape;
            guard.depth += 1;
            prototype = data.proto();
        }
        Some(guard)
    }

    /// Whether the chain from the prototype `link` names still has the shapes the add entry
    /// of `shape` and `atom` recorded, which then holds that proof under the chain's cell.
    /// Every link becomes a prototype first, so a later change to any of them retires the
    /// proof (`Vm::chain_cell`).
    #[cold]
    #[inline(never)]
    fn prove_field_add_chain(&mut self, set: usize, shape: u32, atom: Atom, link: ProtoLink) -> bool {
        // Each entry records one prototype chain; the receiver's must be one of them.
        let entries = self.field_add_sets[set].matching(shape, atom).collect::<smallvec::SmallVec<[usize; FIELD_ADD_CACHE_WAYS]>>();
        for index in entries {
            let cache = *self.field_add_sets[set].entry(index);
            let Some(proved) = self.prove_add_chain(&cache, link) else {
                continue;
            };
            if let Some(cell) = proved {
                let cache = self.field_add_sets[set].entry_mut(index);
                cache.proved = Some(cell);
                cache.proved_prototype = link;
            }
            return true;
        }
        false
    }

    /// Whether the chain from the prototype `link` names still has the shapes `cache`
    /// recorded, with the chain's cell that proof then holds under; `Some(None)` where a link
    /// has no subject, so the walk proves only this add.
    fn prove_add_chain(&mut self, cache: &FieldAddCache, link: ProtoLink) -> Option<Option<crate::heap::ChainCell>> {
        let (proto, depth) = (link.prototype(), cache.prototype_depth);
        // Entries record only non-dictionary source and prototype shapes, a shape's
        // dictionary state is fixed when it is created, and shape compaction clears these
        // caches, so a matching shape needs no dictionary probe here or along the chain.
        let chain = self.field_add_chains.get(cache.chain, usize::from(depth));
        if !self.field_add_prototype_chain_matches(proto, chain) {
            return None;
        }
        // Marking a link that was no prototype yet changes no layout, so the walk made before
        // it holds under the cell made after.
        Some(self.chain_cell(proto, depth))
    }

    /// A keyed `[[Set]]` adding the plain name `atom` (`plain_named_key`) to `object` by the
    /// keyed add entry of its shape (`keyed_adds`), as a named site's cached add makes it
    /// (`try_cached_field_add`). `false` changes nothing.
    pub(super) fn keyed_add_hit(&mut self, object: Value, atom: Atom, value: Value) -> bool {
        let Some(data) = self.heap.object_record(object) else {
            return false;
        };
        // Entries record no dictionary source, which `exact_shape` never names.
        let (shape, link) = (data.exact_shape(), data.proto_link());
        if !data.admits_cached_add() || data.has_side_descriptors() || !self.field_add_atom_eligible(atom) {
            return false;
        }
        let index = keyed_add_index(shape, atom);
        let cache = self.keyed_adds[index];
        if cache.source_shape != shape || cache.atom != atom || self.is_global_object(object) {
            return false;
        }
        if !cache.proof_holds(link) {
            let Some(proved) = self.prove_add_chain(&cache, link) else {
                return false;
            };
            if let Some(cell) = proved {
                self.keyed_adds[index].proved = Some(cell);
                self.keyed_adds[index].proved_prototype = link;
            }
        }
        self.apply_cached_field_add(object, atom, shape, cache.target_shape, cache.slot, value);
        true
    }

    /// The source shape and prototype proof a keyed add of `atom` to `object` would record
    /// (`record_keyed_add`), taken before the add.
    pub(super) fn keyed_add_guard(&self, object: Value, atom: Atom) -> Option<(u32, AddPrototypeGuard)> {
        let data = self.heap.object_record(object)?;
        let shape = data.shape();
        if self.shape_is_dictionary(shape) || self.shape_slot(shape, atom).is_some() || !self.field_add_atom_eligible(atom) {
            return None;
        }
        Some((shape, self.add_prototype_guard(object, PropertyKey::string(atom))?))
    }

    /// Remembers the add of `atom` that moved `object` from `shape`, proved by `guard`
    /// (`keyed_add_guard`), as `keyed_add_hit` replays it: an append at the source shape's
    /// end to a non-dictionary shape.
    pub(super) fn record_keyed_add(&mut self, object: Value, atom: Atom, shape: u32, guard: AddPrototypeGuard) {
        let Some(target_shape) = self.heap.object_record(object).map(Object::shape) else {
            return;
        };
        let Some(slot) = self.shape_slot(target_shape, atom) else {
            return;
        };
        if target_shape == shape
            || self.shape_is_dictionary(target_shape)
            || slot != self.shapes[shape as usize].storage_len()
        {
            return;
        }
        let Ok(slot) = u32::try_from(slot) else {
            return;
        };
        let guarded = self.guarded_field_add(&guard);
        self.keyed_adds[keyed_add_index(shape, atom)] =
            FieldAddCache { atom, source_shape: shape, target_shape, slot, ..guarded };
    }

    #[inline(always)]
    fn field_add_prototype_chain_matches(&self, mut prototype: Value, expected: &[u32]) -> bool {
        for expected_shape in expected {
            let Some(data) = self.heap.object_record(prototype) else {
                return false;
            };
            if data.shape() != *expected_shape
                || data.has_exotic_access()
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
    /// a hit is the same store either way; a miss changes nothing. `store_owed` is false when
    /// `cached_own_store_hit` already probed the store half and missed, leaving the add half.
    pub(super) fn set_field_cache_hit(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        value: Value,
        site: u16,
        store_owed: bool,
    ) -> bool {
        if !(self.specialized && p.specialized) || self.is_private_name(atom) {
            return false;
        }
        let Some(data) = self.heap.object(object) else {
            return false;
        };
        if data.has_exotic_access() {
            return false;
        }
        let site = self.field_cache_index(site);
        let (shape, link, extensible) = (data.shape(), data.proto_link(), data.is_extensible());
        (store_owed && self.try_cached_field_store(object, atom, shape, value, site))
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
        let cache = self.field_caches.get(site);
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
        #[cfg(feature = "profile-field-primitives")]
        self.profile.field_primitives.hit(crate::profile::field_primitives::Path::GeneralStore);
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
            self.proxy_set_mode(
                p,
                target,
                handler,
                object,
                PropertyKey::string(atom),
                value,
                strict,
            )?
        } else {
            self.set_property_with_receiver(p, object, atom, value, object)?
        };
        if written || !strict {
            Ok(())
        } else {
            Err(self.rejected_assignment_error(p, object, atom, false))
        }
    }

    pub fn prototype_chain_contains_proxy(&self, object: Value) -> bool {
        let mut current = self.heap.object(object).map(|data| data.proto());
        while let Some(value) = current.filter(|value| !value.is_null()) {
            if matches!(self.heap.get(value), Some(Cell::Proxy { .. })) {
                return true;
            }
            current = self.heap.object(value).map(|data| data.proto());
        }
        false
    }

    pub fn property_accessor(
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
            object = self.heap.object(object)?.proto();
            if object.is_null() {
                return None;
            }
        }
    }

    pub(super) fn inherited_write_blocked(&self, object: Value, atom: Atom) -> bool {
        let mut object = self.heap.object(object).map(|data| data.proto());
        while let Some(current) = object.filter(|value| !value.is_null()) {
            if let Some(attributes) = self.property_attributes(current, PropertyKey::string(atom)) {
                return !attributes.accessor && !attributes.writable;
            }
            object = self.heap.object(current).map(|data| data.proto());
        }
        false
    }
    /// `callable_write` for a slot a matching cached shape proved present: an ordinary
    /// object's old value is read in place rather than through `heap.object`.
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
                .heap.object(object)
                .and_then(|object| self.heap.property_get_ref(object, slot))
                .is_some_and(|old| self.is_function(old))
    }
    /// `cache` with its slot in the spill run where the class-keyed field it names spills (`spilled_slot`): an own
    /// entry reads its receiver's class cell, a held entry its holder cell (the class the read goes to), so the
    /// spill slot is that class's `spill_from`. A shape-keyed or an inline field keeps its slot.
    fn spill_encoded_entry(&self, mut cache: FieldCache) -> FieldCache {
        if !is_class_key(cache.receiver) || is_spilled_slot(cache.slot) {
            return cache;
        }
        let spill_from = if cache.holder == NO_FIELD_HOLDER {
            self.heap.class_spill_from(cache.receiver & !CLASS_KEY)
        } else {
            self.heap.live_spill_from(cache.holder_cell)
        };
        if let Some(from) = spill_from.filter(|&from| cache.slot >= from) {
            cache.slot = spilled_slot(from, cache.slot - from);
        }
        cache
    }

    /// `add` with its slot in the spill run where a class-keyed add's target field spills (`spilled_slot`): the
    /// source class's region decides the spill slot of the target shape (`Heap::shape_spill_from`). A shape-keyed
    /// add, or one whose target fits, keeps its slot.
    fn spill_encoded_add(&self, mut add: FieldAddCache) -> FieldAddCache {
        if !is_class_key(add.source_shape) || is_spilled_slot(add.slot) {
            return add;
        }
        let region = crate::heap::layouts::class_region(add.source_shape & !CLASS_KEY);
        if let Some(from) = self.heap.shape_spill_from(region, add.target_shape).filter(|&from| add.slot >= from) {
            add.slot = spilled_slot(from, add.slot - from);
        }
        add
    }

    pub(super) fn record_field_cache(&mut self, site: usize, cache: FieldCache) {
        let cache = self.spill_encoded_entry(cache);
        if !self.field_cache_records(site) {
            return;
        }
        self.note_field_cache_write(site);
        // SAFETY: compiler-produced sites index exactly-sized cache vectors.
        let table_index = unsafe { self.megamorphic_field_indices.get_unchecked(site) };
        if table_index != NO_MEGAMORPHIC_FIELD {
            // SAFETY: every non-sentinel index is written when its table is pushed.
            let entries = unsafe {
                self.megamorphic_fields
                    .get_unchecked_mut(table_index as usize)
            };
            entries.insert(cache);
            self.set_primary_field_cache(site, cache);
            return;
        }
        let entry = self.field_caches.get(site);
        if (entry.receiver == cache.receiver && entry.holder == cache.holder)
            || entry.receiver == NO_FIELD_RECEIVER
        {
            self.set_primary_field_cache(site, cache);
            return;
        }
        let table_index = self.megamorphic_fields.len() as u32;
        self.megamorphic_fields
            .push(FieldCacheSet::with_pair(entry, cache));
        // SAFETY: compiler-produced sites index exactly-sized cache vectors.
        unsafe {
            self.megamorphic_field_indices.set_unchecked(site, table_index);
        }
        self.set_primary_field_cache(site, cache);
    }
    /// Makes `cache` the site's primary entry. The primary entry is the
    /// one the specializers' site rows read (`staged::entry::site_read`), so it is always the
    /// latest the interpreter resolved: a receiver whose shape moved on is read natively again
    /// after one leave, where a primary entry pinned to the first shape left on every read.
    /// The megamorphic set, which holds every entry, still answers the interpreter's misses.
    fn set_primary_field_cache(&mut self, site: usize, cache: FieldCache) {
        self.field_caches.set(site, cache);
    }
    #[inline(always)]
    pub(super) fn megamorphic_field_cache(
        &self,
        site: usize,
        receiver: u32,
        holder: u32,
    ) -> Option<FieldCache> {
        // SAFETY: compiler-produced sites index exactly-sized cache vectors;
        // every non-sentinel index was installed together with its table.
        let table_index = unsafe { self.megamorphic_field_indices.get_unchecked(site) };
        if table_index == NO_MEGAMORPHIC_FIELD {
            return None;
        }
        unsafe {
            self.megamorphic_fields
                .get_unchecked(table_index as usize)
                .get(receiver, holder)
        }
    }
}

/// The `Vm::keyed_adds` entry of an add of `atom` to a receiver of `shape`: the high bits of
/// their Fx hash.
fn keyed_add_index(shape: u32, atom: Atom) -> usize {
    use std::hash::Hasher;
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write_u64(u64::from(shape) << u32::BITS | u64::from(atom));
    (hasher.finish() >> (u64::BITS - super::KEYED_ADD_INDEX_BITS)) as usize
}


/// Whether the prototype `holder` has the exact shape `shape` a field cache recorded for a
/// holder. Caches never record a module namespace (`immediate_prototype_data_field`), whose
/// shape no other object shares, so a match is never a namespace and needs no test of it.
#[inline(always)]
fn cached_holder_matches(holder: &crate::heap::Object, shape: u32) -> bool {
    let matches = holder.exact_shape() == shape;
    debug_assert!(!matches || !holder.is_module_namespace(), "a field cache names no module namespace");
    matches
}

impl Vm {
    /// The shape table as the heap registers it (`Heap::track_slot_representations`).
    pub(super) fn shape_layouts(&self) -> Vec<(usize, Option<(u32, usize)>)> {
        super::shape_layouts(&self.shapes)
    }

    /// The field sites' polymorphism, for a diagnostics run (`QUENCH_TRACE=js-jit`): the
    /// polymorphic sites by how many receiver shapes they recorded, how many name only own
    /// fields and of those how many find the field at one slot in every shape (a layout whose
    /// subclasses share their parent's prefix reads them at a fixed offset), and how many name
    /// prototype-held fields.
    pub(super) fn field_site_report(&self) -> String {
        let (mut sizes, mut own, mut one_slot, mut holders, mut overflowed) = ([0_usize; 4], 0, 0, 0, 0);
        for set in &self.megamorphic_fields {
            let entries: Vec<FieldCache> = match &set.overflow {
                Some(overflow) => { overflowed += 1; overflow.values().copied().collect() }
                None => set.recorded().copied().collect(),
            };
            let bucket = match entries.len() { 0..=2 => 0, 3..=4 => 1, 5..=16 => 2, _ => 3 };
            sizes[bucket] += 1;
            if entries.iter().all(|entry| entry.holder == NO_FIELD_HOLDER) {
                own += 1;
                if entries.windows(2).all(|pair| pair[0].slot == pair[1].slot) { one_slot += 1; }
            } else {
                holders += 1;
            }
        }
        // The representation each own field a cache entry names holds across the live objects
        // of its shape, the join of their values' classes (`SlotType`).
        use crate::typed_ir::SlotType;
        let mut classes: std::collections::HashMap<(u32, u16), SlotType> = std::collections::HashMap::new();
        self.heap.for_each_live_object_record(|object| {
            let shape = object.exact_shape();
            if shape != object.shape() { return; }
            let Some(len) = self.shapes.get(shape as usize).map(|shape| shape.storage_len()) else { return };
            for slot in 0..len {
                // SAFETY: an exact, non-dictionary shape holds `len` slots.
                let value = unsafe { self.heap.property_get_flat(object, slot) };
                if value.is_deleted() { continue; }
                let class = SlotType::of(value);
                let joined = classes.entry((shape, slot as u16)).or_insert(class);
                *joined = joined.join(class);
            }
        });
        let mut reps = [0_usize; SlotType::ALL.len()];
        let own_entries = (0..self.field_caches.len()).map(|index| self.field_caches.get(index)).chain(self.megamorphic_fields.iter().flat_map(|set| set.recorded().copied().collect::<Vec<_>>()))
            .filter(|cache| cache.receiver != NO_FIELD_RECEIVER && cache.holder == NO_FIELD_HOLDER);
        for cache in own_entries {
            reps[classes.get(&(cache.receiver, cache.slot)).copied().unwrap_or(SlotType::None) as usize] += 1;
        }
        let classes = SlotType::ALL.iter().map(|class|
            format!("\"{}\":{}", class.label(), reps[*class as usize])).collect::<Vec<_>>().join(",");
        let reps = format!("QUENCH_FIELD_REPS {{{classes},\"unrepresented_slots\":{}}}", self.heap.unrepresented_slots());
        reps + "\n" + &format!(
            "QUENCH_FIELD_SITES {{\"polymorphic\":{},\"shapes_2\":{},\"shapes_3_4\":{},\"shapes_5_16\":{},\"shapes_17_plus\":{},\"overflowed\":{overflowed},\"own_only\":{own},\"own_one_slot\":{one_slot},\"with_holders\":{holders}}}",
            self.megamorphic_fields.len(), sizes[0], sizes[1], sizes[2], sizes[3],
        )
    }
}

/// The slot type seed of `atom`, added to an instance of the class whose constructor is `function`
/// of `p`, from its declared field type (`DeclaredTypes::declared_field`): the class's constructor
/// carries the class's source span, whose start names the class. `None` where the declaration
/// holds no fact: no declaration, an open set (`ANY_LANGUAGES`), an empty set, or a set whose
/// slot class is `SlotType::Tagged` (exactly `Type::ANY`, which a seed would only widen to).
fn declared_field_type(p: &ResidualProgram, function: u32, atom: Atom) -> Option<crate::typed_ir::Type> {
    let declared = p.declared_types.as_deref()?;
    let class_start = p.function_get(function as usize).and_then(|body| body.source_span).map(|span| span.start)?;
    let bits = declared.declared_field(class_start, &p.atoms[atom as usize])?;
    if bits == crate::typed_ir::ANY_LANGUAGES || bits == crate::typed_ir::Languages::EMPTY.bits() {
        return None;
    }
    let ty = crate::typed_ir::Type::languages(bits);
    (crate::typed_ir::SlotType::from_type(ty) != crate::typed_ir::SlotType::Tagged).then_some(ty)
}

/// The layout id of `holder` where it names exactly `shape`, else `NO_FIELD_HOLDER_LAYOUT`: the
/// id a holder entry keeps (`FieldCache::holder_cell`). Id 0 is the all-zero header and names no
/// holder, so it stays unset; `layouts::auxiliary` of an id is its shape (`layout_shape`).
fn holder_layout_id(holder: &Object, shape: u32) -> u32 {
    let id = holder.layout_id();
    if id != NO_FIELD_HOLDER_LAYOUT && crate::staged::ic::layout_shape(id) == shape { id } else { NO_FIELD_HOLDER_LAYOUT }
}
