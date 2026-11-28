//! Immutable creation names share the shape's lifetime and property transitions.
use super::*;

// V8 JSFunction::kNameDescriptorIndex: length precedes the native name accessor.
const FUNCTION_NATIVE_NAME_DESCRIPTOR_INDEX: usize = 1;

/// Direct-mapped entries of `FunctionNameShapes`: a power of two that holds the distinct
/// closure names a request path makes.
const FUNCTION_NAME_SHAPE_ENTRIES: usize = 1 << 8;

/// Memoized `append_function_name` answers: the shape a closure's shape takes with a name.
/// Closures of one site take the same name on the same shape, but that shape has a named
/// child per site, which defeats `Shape::last_child`. Entries name shapes, so they are
/// cleared with the transition index and whenever a collection renumbers shapes.
#[derive(Default)]
pub(super) struct FunctionNameShapes {
    entries: super::shape_memo::DirectMappedTable<(u32, Atom, u32), FUNCTION_NAME_SHAPE_ENTRIES>,
}


impl FunctionNameShapes {
    fn index(shape: u32, name: Atom) -> usize {
        (name ^ shape.rotate_left(FUNCTION_NAME_SHAPE_ENTRIES.trailing_zeros())) as usize
            & (FUNCTION_NAME_SHAPE_ENTRIES - 1)
    }

    fn get(&self, shape: u32, name: Atom) -> Option<u32> {
        self.entries
            .get(Self::index(shape, name))
            .filter(|&(base, atom, _)| base == shape && atom == name)
            .map(|(_, _, named)| named)
    }

    fn record(&self, shape: u32, name: Atom, named: u32) {
        self.entries.set(Self::index(shape, name), (shape, name, named));
    }

    pub(super) fn clear(&self) {
        self.entries.clear();
    }
}

/// A memo of `Vm::constructed_shape`: the shape an object of `shape` takes when
/// `constructor` constructs it. The answer depends only on that key, the constructor's
/// immutable original name and the shape tree, so it holds until a collection, which can
/// renumber shapes or reuse a dead constructor's cell and so clears every entry.
#[derive(Clone, Copy)]
pub(super) struct ConstructedShape {
    shape: u32,
    constructor: Value,
    class: u32,
    new_target_is_base: bool,
    constructed: u32,
}

/// Entries in the direct-mapped `Vm::constructed_shapes` memo; a power of two.
pub(super) const CONSTRUCTED_SHAPE_ENTRIES: usize = 64;

impl ConstructedShape {
    /// An entry no construction matches: no constructor is `undefined`.
    pub(super) const EMPTY: Self = Self {
        shape: u32::MAX,
        constructor: Value::UNDEFINED,
        class: 0,
        new_target_is_base: false,
        constructed: u32::MAX,
    };

    #[inline]
    fn cache_index(&self) -> usize {
        // Heap handles differ in their low payload bits; the shape separates one
        // constructor's bases.
        let handle = self.constructor.heap_index().unwrap_or_default() as usize;
        (handle ^ self.shape as usize) & (CONSTRUCTED_SHAPE_ENTRIES - 1)
    }

    #[inline]
    fn same_construction(&self, other: &Self) -> bool {
        self.shape == other.shape
            && self.constructor == other.constructor
            && self.class == other.class
            && self.new_target_is_base == other.new_target_is_base
    }
}

/// The metadata transitions `shape`'s chain records, which a shape rebuilt without its chain
/// replays first: its function name, a reconfigured name or length, what constructed it.
pub(super) fn metadata_transitions(shapes: &[Shape], shape: u32) -> impl Iterator<Item = ShapeTransition> {
    let (function_name, constructed_name, name_reconfigured) = shape_names(shapes, shape);
    [
        function_name.map(|name| ShapeTransition::FunctionName { name }),
        name_reconfigured.then_some(ShapeTransition::FunctionNameReconfigured),
        shape_function_length_reconfigured(shapes, shape).then_some(ShapeTransition::FunctionLengthReconfigured),
        constructed_name.map(|(constructor, class, new_target_is_base)| ShapeTransition::Constructed { constructor, class, new_target_is_base }),
    ].into_iter().flatten()
}

pub(super) fn shape_names(shapes: &[Shape], shape: u32) -> (Option<Atom>, Option<(Value, u32, bool)>, bool) {
    let (mut function, mut constructed) = (None, None);
    let mut current = Some(shape);
    while let Some(id) = current {
        let shape = &shapes[id as usize];
        match shape.transition {
            ShapeTransition::FunctionName { name } if function.is_none() => function = Some(name),
            ShapeTransition::Constructed {
                constructor,
                class,
                new_target_is_base,
            } if constructed.is_none() => constructed = Some((constructor, class, new_target_is_base)),
            _ => {}
        }
        current = shape.parent();
    }
    (function, constructed, shapes[shape as usize].facts.name_reconfigured())
}

pub(super) fn shape_function_length_reconfigured(shapes: &[Shape], shape: u32) -> bool {
    shapes[shape as usize].facts.length_reconfigured()
}

impl Vm {
    pub(super) fn capture_function_name(&mut self, function: Value, name: Value) {
        let Some(name) = self.heap.string(name) else {
            return;
        };
        let name = name.clone();
        let name = self.intern_js_atom(&name);
        self.capture_function_name_for_atom(function, name);
    }

    /// `capture_function_name` of a name already interned as `name`.
    pub(super) fn capture_function_name_for_atom(&mut self, function: Value, name: Atom) {
        if self.original_function_name(function) == Some(name) {
            return;
        }
        self.capture_function_name_atom(function, name);
    }

    fn capture_function_name_atom(&mut self, function: Value, name: Atom) {
        let Some(shape) = self.heap.object(function).map(|object| object.shape()) else { return; };
        // A first original name makes the function record what it constructs.
        if shape_names(&self.shapes, shape).0.is_none() {
            self.constructed_shapes.fill(ConstructedShape::EMPTY);
        }
        self.append_function_name(function, shape, name);
    }

    /// Names `function` along the edge cache `site` recorded for `name` (`record_function_name_edge`):
    /// the staged naming row, which proves the function's before-shape, plain storage, empty name and
    /// the store gate. False changes nothing and leaves the naming to the general path.
    pub(super) fn named_by_transition(&mut self, function: Value, name: Atom, site: u16) -> bool {
        let Some(record_id) = function.direct_function_index() else {
            return false;
        };
        let Some(shape) = self.heap.object(function).map(|object| object.shape()) else {
            return false;
        };
        let Some((_, cache)) = self.field_add_entry(self.field_cache_index(site), shape, name) else {
            return false;
        };
        let transition = cache.transition();
        let (empty, word) = (self.empty_string_word().to_bits(), self.atom_string(name));
        // SAFETY: a direct function value names a live function record, whose shape the row proves.
        unsafe { concrete_set_function_name(&mut self.heap, record_id, transition, empty, word) }
    }

    /// The naming cache `site` recorded for `name` that compiled code may run (`staged::naming`): its edge
    /// and the word of the name's string, which the interpreter's own run found (`atom_string`). `None`
    /// where the site recorded none.
    pub fn native_function_name(&self, site: u16, name: Atom) -> Option<(Transition, u64)> {
        let word = self.heap.atom_string(name, true)?;
        let edge = self.native_field_add(site, name, false)?;
        Some((edge.transition, word.to_bits()))
    }

    /// Records the naming of `function` to `name` as the edge of cache `site` from `before` (its shape
    /// then, with the `name` slot) to the shape it has now, where a row can repeat it: both shapes are
    /// plain layouts, and `before` already records an original name, so the naming moves no
    /// construction memo (`capture_function_name_atom`).
    pub(super) fn record_function_name_edge(&mut self, site: u16, function: Value, name: Atom, ((before, layout), slot): ((u32, u32), usize)) {
        let Some(after) = self.heap.object(function).map(|object| object.shape()) else {
            return;
        };
        let Ok(slot) = u32::try_from(slot) else {
            return;
        };
        let plain = |vm: &Self, shape: u32| !vm.shape_is_dictionary(shape);
        if after != before && plain(self, before) && plain(self, after) && shape_names(&self.shapes, before).0.is_some() {
            let layouts = Object::add_layouts(layout, after);
            let edge = FieldAddCache { atom: name, source_shape: before, target_shape: after, slot, layouts, ..FieldAddCache::EMPTY };
            self.record_field_add_cache(self.field_cache_index(site), edge);
        }
    }

    /// `capture_function_name` of a closure `closure_in_realm` just made with the plan's
    /// `name`, whose atom is the name its string spells, or none for the empty name. Its shape
    /// holds no original name yet, and no construction has seen its cell since the last
    /// collection, which cleared `constructed_shapes`, so no memo entry names it.
    pub(super) fn capture_fresh_closure_name(&mut self, function: Value, name: Option<Atom>) {
        let name = name.unwrap_or(self.runtime_atoms.empty);
        let Some(shape) = self.heap.object(function).map(|object| object.shape()) else { return; };
        debug_assert!(shape_names(&self.shapes, shape).0.is_none(), "a fresh closure has no original name");
        self.append_function_name(function, shape, name);
    }

    fn append_function_name(&mut self, function: Value, shape: u32, name: Atom) {
        let named = self.function_name_shape(shape, name);
        self.heap.set_object_shape(function, named);
    }

    /// `shape` with the original function name `name` recorded (`ShapeTransition::FunctionName`),
    /// through the transitions it remembers (`function_name_shapes`).
    pub(super) fn function_name_shape(&mut self, shape: u32, name: Atom) -> u32 {
        if let Some(named) = self.function_name_shapes.get(shape, name) {
            return named;
        }
        let named = self.append_cached_shape_transition(shape, ShapeTransition::FunctionName { name });
        self.function_name_shapes.record(shape, name, named);
        named
    }

    pub(super) fn set_function_debug_name(&mut self, function: Value, name: Atom) {
        if matches!(self.heap.function(function), Some(FunctionRecord { kind: FunctionKind::User(..), .. }))
            && self.original_function_name(function).is_some_and(|original| self.atom_units_equal_str(original, ""))
        {
            self.capture_function_name_atom(function, name);
        }
    }

    pub(super) fn default_function_name_info(&self, function: Value, key: PropertyKey) -> bool {
        if !matches!(key, PropertyKey::String(atom) if self.atom_units_equal_str(atom, "name"))
            || !self.heap.is_function_value(function)
        { return false; }
        let Some(object) = self.heap.object(function) else { return false; };
        let (name, _, reconfigured) = shape_names(&self.shapes, object.shape());
        name.is_some() && !reconfigured
            && self.shape_entries(object.shape()).nth(FUNCTION_NATIVE_NAME_DESCRIPTOR_INDEX)
                .is_some_and(|(candidate, _)| candidate == key)
            // The prototype-lookup flag is record data.
            && (!object.was_used_as_prototype() || object.prototype_lookup_ready())
    }

    pub(super) fn reconfigure_function_name(&mut self, function: Value, key: PropertyKey) {
        if !self.heap.is_function_value(function) { return; }
        let Some(object) = self.heap.object(function) else { return; };
        let (name, _, reconfigured) = shape_names(&self.shapes, object.shape());
        if name.is_none() { return; }
        let transition = match key {
            PropertyKey::String(atom) if self.atom_units_equal_str(atom, "name") && !reconfigured => ShapeTransition::FunctionNameReconfigured,
            PropertyKey::String(atom) if self.atom_units_equal_str(atom, "length")
                && !shape_function_length_reconfigured(&self.shapes, object.shape()) => ShapeTransition::FunctionLengthReconfigured,
            _ => return,
        };
        let shape = self.append_cached_shape_transition(object.shape(), transition);
        self.heap.set_object_shape(function, shape);
    }

    /// V8's named native-info load enters prototype lookup mode, whereas
    /// Reflect.get and computed-key reads retain the setup-mode accessor view.
    #[inline(always)]
    pub fn prepare_named_function_info_lookup(&mut self, object: Value, atom: Atom) {
        if !self.reads_function_info(atom) {
            return;
        }
        // An array's own `length` answers the read before any function's could.
        let array_length = atom == self.length_atom
            && self.heap.array(object).is_some_and(|array| !array.is_arguments_object());
        if !array_length {
            self.prepare_function_info_prototypes(object, atom, atom == self.runtime_atoms.name);
        }
    }

    /// Whether a named read of `atom` can enter prototype lookup mode: the function
    /// information keys `name` and `length`. Fixed per read site, so a site may decide it once.
    #[inline(always)]
    pub fn reads_function_info(&self, atom: Atom) -> bool {
        atom == self.runtime_atoms.name || atom == self.length_atom
    }

    // Ordinary property reads never need text inspection or a prototype walk.
    // Keep the exact native-info transition separate from that common path.
    #[inline(never)]
    fn prepare_function_info_prototypes(&mut self, mut object: Value, atom: Atom, name: bool) {
        // Only a function on the chain prepares anything, so a chain without one (a Buffer's
        // `length`) needs none of the property lookups below.
        if !self.chain_reaches_function(object) {
            return;
        }
        let receiver = object;
        while let Some(data) = self.heap.object(object) {
            if self.proxy_parts(object).is_some() { return; }
            if self.own_property(object, atom).is_some() {
                if self.heap.is_function_value(object) {
                    let facts = self.shapes[data.shape() as usize].facts;
                    if facts.function_named() && !if name { facts.name_reconfigured() } else { facts.length_reconfigured() } {
                        let mut current = receiver;
                        while let Some(data) = self.heap.object(current) {
                            // The prototype-lookup flag is record data.
                            if self.proxy_parts(current).is_some() || data.prototype_lookup_ready() { break; }
                            let next = data.proto();
                            self.heap.object_mut(current).expect("lookup object").prepare_prototype_lookup();
                            current = next;
                        }
                    }
                }
                return;
            }
            if self.property_attributes(object, PropertyKey::string(atom)).is_some_and(|attributes| attributes.accessor) { return; }
            object = data.proto();
        }
    }

    /// Whether the prototype chain from `object` holds a function before it ends or reaches a
    /// proxy, where `prepare_function_info_prototypes` stops.
    fn chain_reaches_function(&self, mut object: Value) -> bool {
        while let Some(data) = self.heap.object(object) {
            if self.proxy_parts(object).is_some() {
                return false;
            }
            if self.heap.is_function_value(object) {
                return true;
            }
            object = data.proto();
        }
        false
    }

    pub(super) fn original_function_name(&self, function: Value) -> Option<Atom> {
        let object = self.heap.object(function)?;
        shape_names(&self.shapes, object.shape()).0
    }

    /// The source/debug name can differ from the public creation name: dynamic
    /// Function constructors expose "anonymous" while their source name is empty.
    pub(super) fn original_debug_name(&self, function: Value) -> Option<JsString> {
        if let Some(FunctionRecord { kind: FunctionKind::User(program, id), .. }) = self.heap.function(function)
            && self.programs.is_function_constructor_body(*program, *id) { return None; }
        self.original_function_name(function).map(|name| self.atom_value(name))
    }

    /// V8's debug name reads data properties but never invokes a name accessor.
    pub(super) fn debug_function_name(&self, function: Value) -> Option<JsString> {
        let name_key = self.find_atom(Self::atom_hash_str("name"), |atom| {
            self.atom_units_equal_str(atom, "name")
        });
        if let Some(name_key) = name_key
            && let Some(name) = self
                .diagnostic_data_property(function, PropertyKey::string(name_key))
                .and_then(|value| self.heap.string(value))
            {
                return Some(name.clone());
            }
        self.original_debug_name(function)
    }

    /// The shape a construction of class `class` by `constructor` gives an object of `shape`: its `Constructed` root,
    /// seeded with the union of `class` (`seed_union_domains`) when the root is a base's own.
    fn constructed_shape(
        &mut self,
        shape: u32,
        constructor: Value,
        class: u32,
        new_target_is_base: bool,
    ) -> u32 {
        let entry = ConstructedShape { shape, constructor, class, new_target_is_base, constructed: shape };
        let index = entry.cache_index();
        let cached = self.constructed_shapes[index];
        if cached.same_construction(&entry) {
            return cached.constructed;
        }
        let constructed = self.derive_constructed_shape(shape, constructor, class, new_target_is_base);
        self.seed_union_domains(constructor, class, constructed);
        self.constructed_shapes[index] = ConstructedShape { constructed, ..entry };
        constructed
    }

    /// The shape of a construction by `constructor`: the chain of `shape` with its `Constructed` transition. Keyed by the
    /// constructor value alone, so an anonymous constructor gets its root too (the allocator's union cell is keyed by the
    /// class, not its name); the name is read lazily where provenance reads it (`constructed_name`).
    fn derive_constructed_shape(
        &mut self,
        shape: u32,
        constructor: Value,
        class: u32,
        new_target_is_base: bool,
    ) -> u32 {
        if shape_names(&self.shapes, shape).1 == Some((constructor, class, new_target_is_base)) {
            return shape;
        }
        self.append_cached_shape_transition(
            shape,
            ShapeTransition::Constructed {
                constructor,
                class,
                new_target_is_base,
            },
        )
    }

    /// The shape a construction by `constructor` gives an empty object of `shape` before its body
    /// runs (`capture_constructed_name`, `new.target` the constructor itself): an ahead image's
    /// shape recipe of a constructed object starts there (`aot_image::symbols::shape`).
    pub fn constructed_root_shape(&mut self, shape: u32, constructor: Value) -> u32 {
        let class = self.ordinary_class(constructor);
        self.constructed_shape(shape, constructor, class, true)
    }

    /// Records on `object` the `Constructed` root of a construction of class `class` by `constructor`. A construction
    /// with no construct site passes `ordinary_class(constructor)`.
    pub(super) fn capture_constructed_name(
        &mut self,
        object: &mut Object,
        constructor: Value,
        class: u32,
        new_target_is_base: bool,
    ) {
        let shape = self.constructed_shape(object.shape(), constructor, class, new_target_is_base);
        self.heap.set_detached_object_shape(object, shape);
    }

    /// Records on the object `value` the `Constructed` root of a construction of class `class` by `constructor`
    /// (`capture_constructed_name` for an object already allocated).
    pub fn capture_constructed_value(
        &mut self,
        value: Value,
        constructor: Value,
        class: u32,
        new_target_is_base: bool,
    ) {
        let Some(shape) = self.heap.object(value).map(|object| object.shape()) else {
            return;
        };
        let shape = self.constructed_shape(shape, constructor, class, new_target_is_base);
        self.heap.set_object_shape(value, shape);
    }

    pub(crate) fn embedding_original_function_name(
        &self,
        root: RootId,
    ) -> Result<Option<JsString>, JsError> {
        let value = self.embedding_value(root)?;
        if !matches!(self.heap.function(value), Some(FunctionRecord { kind, .. }) if !matches!(kind, FunctionKind::Native(Native::FunctionBoundCall)))
        {
            return Ok(None);
        }
        Ok(self
            .original_function_name(value)
            .map(|name| self.atom_value(name)))
    }

    /// A function's existing code kind and creation realm own its factory origin.
    /// Bound functions have no such ordinary-function constructor projection.
    fn function_factory_constructor(&self, value: Value) -> Option<Value> {
        let record = self.heap.function(value)?;
        let realm = self.record_realm(record);
        let intrinsic = match record.kind {
            FunctionKind::User(program, id) => {
                let program = self.programs.get(program)?;
                let function = program.functions.get(id as usize)?.header();
                Native::function_constructor(function.is_async, function.is_generator)
            }
            FunctionKind::Native(Native::FunctionBoundCall) => return None,
            FunctionKind::Native(_) => Native::Function,
        };
        self.realm
            .intrinsics
            .function_constructors
            .get(&(realm, intrinsic))
            .copied()
    }

    pub(crate) fn embedding_constructed_name(
        &self,
        root: RootId,
    ) -> Result<Option<JsString>, JsError> {
        Ok(self.constructed_name(self.embedding_value(root)?))
    }

    pub(super) fn constructed_name(&self, value: Value) -> Option<JsString> {
        let object = self.heap.object(value)?;
        if object.was_used_as_prototype() {
            return None;
        }
        if let Some((constructor, new_target_is_base)) = shape_names(&self.shapes, object.shape()).1 {
            // An anonymous constructor's root names nothing: its empty name reads as no name, as it did before the root was made.
            return new_target_is_base
                .then(|| self.debug_function_name(constructor))
                .flatten()
                .filter(|name| !name.units().is_empty());
        }
        if let Some(constructor) = self.function_factory_constructor(value) {
            return self.debug_function_name(constructor);
        }
        // These iterator maps have inaccessible intrinsic constructors. Their
        // immutable names derive from the existing iterator kind.
        let name = match self.heap.iterator(value) {
            Some(IteratorRecord {
                kind: IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::MapEntries,
                ..
            }) => "MapIterator",
            Some(IteratorRecord {
                kind: IteratorKind::SetValues | IteratorKind::SetEntries,
                ..
            }) => "SetIterator",
            Some(IteratorRecord {
                kind: IteratorKind::String,
                ..
            }) => "StringIterator",
            _ => return None,
        };
        Some(JsString::from_str(name))
    }

}
