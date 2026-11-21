use super::*;

/// What a global name read finds (`Vm::global_name_read`): a value, with the hint naming the
/// global object's own data field that held it, an uninitialized lexical binding, or nothing
/// the lane's read takes.
pub(super) enum GlobalNameRead {
    Value(Value, Option<FieldCache>),
    Uninitialized,
    Miss,
}

/// The receiver an inherited field entry's test reads (`Vm::inherited_receiver`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InheritedReceiver {
    Record,
    Function,
    Builtin,
}

impl<H: Host> Vm<H> {
    /// The object an inherited entry's receiver test reads, and its kind: an object record;
    /// a function, which an entry answers only where its shape holds the name
    /// (`InheritedFieldCache::function_links`); or an array or ordinary builtin (a
    /// collection, date, regexp, promise, iterator or generator), whose shape answers every
    /// name but the exotic ones (`inherited_entry_answers`), as `shape_property_lookup`
    /// reads them.
    #[inline(always)]
    pub(super) fn inherited_receiver(&self, value: Value) -> Option<(&Object, InheritedReceiver)> {
        if let Some(record_id) = value.direct_object_index() {
            return Some((self.heap.object_record_by_id(record_id), InheritedReceiver::Record));
        }
        if let Some(function) = self.heap.function(value) {
            return Some((&function.object, InheritedReceiver::Function));
        }
        let builtin: Option<&Object> = self.heap.array(value).map(|array| &**array);
        // A typed array inherits its methods two links up (`%TypedArray%.prototype`); its numeric
        // names are its own elements, which `inherited_entry_answers` keeps from any entry.
        builtin
            .or_else(|| self.heap.ordinary_builtin_object(value))
            // A typed array's shape holds every own name but its canonical numeric strings,
            // which `inherited_entry_answers` refuses (`AtomClass::TYPED_ARRAY_INDEX`): its
            // prototype methods (`u8.fill`, `f64.indexOf`) take the inherited entry.
            .or_else(|| match self.heap.get(value) {
                Some(cell @ Cell::TypedArray { .. }) => cell.object(),
                _ => None,
            })
            .map(|object| (object, InheritedReceiver::Builtin))
    }

    /// Whether inherited entry `index` answers a receiver of `kind`: a builtin's own lookup of
    /// a private, virtual-field, index or canonical numeric name (a typed array's element) is not
    /// its shape's (`shape_answers`).
    #[inline(always)]
    pub(super) fn inherited_entry_answers(&self, kind: InheritedReceiver, index: u32) -> bool {
        kind != InheritedReceiver::Builtin
            || self.heap.dependencies().inherited_get(index).is_some_and(|cache| {
                !self.atom_class(cache.atom).intersects(AtomClass::PRIVATE | AtomClass::VIRTUAL_FIELD | AtomClass::ARRAY_INDEX | AtomClass::TYPED_ARRAY_INDEX)
            })
    }

    /// A global name read as the lane takes it (`dispatch_fast::global_name_refresh_miss`):
    /// an initialized global lexical binding of the name, else the global object's own data
    /// field with the hint that names it, else the site's cached field.
    pub(super) fn global_name_read(&mut self, code: &ResidualProgram, atom: Atom, site: u16) -> GlobalNameRead {
        let globals = self.realm.globals;
        if self.realm.global_lexical_declarations.contains(&atom) {
            return self.realm.global_lexical_bindings.get(&atom).copied().filter(|value| !value.is_deleted())
                .map_or(GlobalNameRead::Uninitialized, |value| GlobalNameRead::Value(value, None));
        }
        if self.global_name_hintable(atom)
            && let Some(hint) = self.own_data_field_hint::<false>(globals, atom)
            && let Some(value) = self.record_field_entry_hit(hint, globals)
        {
            return GlobalNameRead::Value(value, Some(hint));
        }
        match self.cached_field_hit(code, globals, site) {
            object::FieldCacheProbe::Hit(value) => GlobalNameRead::Value(value, None),
            _ => GlobalNameRead::Miss,
        }
    }

    /// The value of the active realm's global lexical binding at `index`, where a hint names it
    /// (`dispatch_fast::lane_load_root_lexical`, `lane_load_name_lexical`): the binding there while
    /// it is `atom`'s and initialized. The index is only a locator, so a stale one, or a binding
    /// in its temporal dead zone, gives `None` and the caller takes its walk or refresh.
    #[inline(always)]
    pub(super) fn global_lexical_at(&self, atom: Atom, index: u16) -> Option<Value> {
        self.realm
            .global_lexical_bindings
            .get_index(usize::from(index))
            .filter(|(bound, value)| **bound == atom && !value.is_deleted())
            .map(|(_, value)| *value)
    }

    /// The store of `value` to the global lexical binding `atom` through its realm index slot, where
    /// `global_lexical_at`'s guard holds and the binding is mutable and initialized: the store the
    /// by-name path makes (`store_global_lexical_assignment`), without its lookup. Otherwise `false`,
    /// and the by-name store settles it: a temporal dead zone, a const, or a stale index.
    pub(super) fn global_lexical_store_at(&mut self, atom: Atom, index: u16, value: Value) -> bool {
        let index = usize::from(index);
        let Some((bound, current)) = self.realm.global_lexical_bindings.get_index(index) else {
            return false;
        };
        if *bound != atom || current.is_deleted() || self.realm.immutable_global_lexical_bindings.contains(&atom) {
            return false;
        }
        self.assign_global_lexical_at(index, value);
        true
    }

    #[inline(always)]
    pub(super) fn field_cache_atom_eligible(&self, atom: Atom) -> bool {
        !self
            .atom_class(atom)
            .intersects(AtomClass::PRIVATE | AtomClass::VIRTUAL_FIELD)
    }

    /// `field_cache_atom_eligible` for a cached add, whose every hit first proves an ordinary
    /// receiver: an ordinary object stores the virtual field names like any other property
    /// (`shape_answers`), so only a private name stays out.
    #[inline(always)]
    pub(super) fn field_add_atom_eligible(&self, atom: Atom) -> bool {
        !self.is_private_name(atom)
    }

    // Shapes prove ordinary data storage only when no exotic or virtual lookup intervenes.
    pub(super) fn shape_property_lookup(&self, value: Value, atom: Atom) -> Option<&Object> {
        let class = self.atom_class(atom);
        if let Some(object) = self.heap.object_record(value) {
            return self.shape_answers(object, class, ReceiverKind::Ordinary);
        }
        if let Some(object) = self.heap.array(value) {
            if class.contains(AtomClass::ARRAY_INDEX) {
                return None;
            }
            return self.shape_answers(object, class, ReceiverKind::Exotic);
        }
        if let Some(function) = self.heap.function(value) {
            return self.shape_property_lookup_function(function, atom, class);
        }
        if let Some(iterator) = self.heap.iterator(value) {
            return self.shape_answers(&iterator.object, class, ReceiverKind::Exotic);
        }
        // A Map's or Set's entries are no named properties: its shape answers every name but its
        // virtual `size`, as a cell's does (`shape_property_lookup_cell`). Without this arm a
        // collection record's reads (`m.get`, `s.has`) were never cached.
        if value.direct_collection_index().is_some() {
            return self.shape_answers(self.heap.object(value)?, class, ReceiverKind::Exotic);
        }
        // A promise's internal slots change no object operation, as for its cell form below.
        if let Some(promise) = self.heap.promise(value) {
            return self.shape_answers(&promise.object, class, ReceiverKind::Ordinary);
        }
        self.shape_property_lookup_cell(self.heap.get(value)?, class)
    }

    /// A function's shape answers what it stores, but not its restricted `caller` and
    /// `arguments`, which reflection computes (`AtomClass::RESTRICTED_FUNCTION_PROPERTY`),
    /// nor its `length`, function information a read prepares outside the shape
    /// (`Vm::prepare_named_function_info_lookup`). The other virtual field names are a
    /// collection's or buffer view's, which no function is: a function stores them as an
    /// ordinary object does, as `Buffer.byteLength`.
    pub(super) fn shape_property_lookup_function<'a>(
        &self,
        function: &'a FunctionRecord,
        atom: Atom,
        class: AtomClass,
    ) -> Option<&'a Object> {
        if atom == self.length_atom || class.contains(AtomClass::RESTRICTED_FUNCTION_PROPERTY) {
            return None;
        }
        self.shape_answers(&function.object, class, ReceiverKind::Ordinary)
    }

    /// Whether `atom` names an element of `object` as a typed array's numeric key, which no
    /// shape stores: the generic [[Set]] converts and writes it.
    pub(super) fn is_typed_array_element(&self, object: Value, atom: Atom) -> bool {
        matches!(self.heap.get(object), Some(Cell::TypedArray { .. })) && self.atom_class(atom).contains(AtomClass::TYPED_ARRAY_INDEX)
    }

    pub(super) fn shape_property_lookup_cell<'a>(
        &self,
        cell: &'a Cell,
        class: AtomClass,
    ) -> Option<&'a Object> {
        let kind = match cell {
            Cell::Promise(_) => ReceiverKind::Ordinary,
            Cell::Proxy { .. } => return None,
            Cell::TypedArray { .. } if class.contains(AtomClass::TYPED_ARRAY_INDEX) => {
                return None;
            }
            _ => ReceiverKind::Exotic,
        };
        self.shape_answers(cell.object()?, class, kind)
    }

    /// `object`'s shape answers `atom` unless a private name, a module namespace, a dictionary
    /// shape or, for an exotic receiver, a virtual field intervenes. Only exotic receivers
    /// answer `length`, `size` and the buffer views' fields outside their shape; an ordinary
    /// object stores them like any other property.
    fn shape_answers<'a>(
        &self,
        object: &'a Object,
        class: AtomClass,
        kind: ReceiverKind,
    ) -> Option<&'a Object> {
        let excluded = match kind {
            ReceiverKind::Ordinary => AtomClass::PRIVATE,
            ReceiverKind::Exotic => AtomClass::PRIVATE | AtomClass::VIRTUAL_FIELD,
        };
        if class.intersects(excluded)
            || object.is_module_namespace()
            || self.shape_is_dictionary(object.shape())
        {
            return None;
        }
        Some(object)
    }
}

/// Whether a receiver can answer names outside its shape (`shape_answers`).
#[derive(Clone, Copy)]
enum ReceiverKind {
    Ordinary,
    Exotic,
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
    /// The data field a receiver any heap cell holds (a function's `f.call`) inherits from its
    /// immediate prototype, among its site's recorded entries (`megamorphic_field_cache`): the
    /// holder tier `polymorphic_field_hit` probes for a record receiver. Every function named
    /// differently has a shape of its own, so such a site records one entry per name.
    pub(super) fn cell_polymorphic_holder_hit(&self, object: Value, site: u16) -> Option<Value> {
        let receiver = self.heap.object(object)?;
        let holder = self.heap.object(receiver.proto())?;
        let entry = self.megamorphic_field_cache(self.field_cache_index(site), receiver.exact_shape(), holder.exact_shape())?;
        (entry.holder != NO_FIELD_HOLDER).then(|| self.heap.property_get(holder, usize::from(entry.slot)))?
    }

    /// Whether the field cache site `site`'s primary entry holds a receiver: a plan-time admission fact for a
    /// field row's direct arm. The arm checks the entry at run time, so a stale answer costs only a decline.
    pub(in crate::vm) fn field_cache_filled(&self, site: u16) -> bool {
        // SAFETY: the active layout reserves every compiler-emitted site, as `cached_field_store_hit` reads it.
        let cache = unsafe { self.field_caches.get_unchecked(self.field_cache_index(site)) };
        cache.receiver != EMPTY_CACHE.receiver
    }
}
