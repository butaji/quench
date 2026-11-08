use super::*;

/// What a global name read finds (`Vm::global_name_read`): a value, with the hint naming the
/// global object's own data field that held it, an uninitialized lexical binding, or nothing
/// the lane's read takes.
pub(super) enum GlobalNameRead {
    Value(Value, Option<FieldCache>),
    Uninitialized,
    Miss,
}

impl<H: Host> Vm<H> {
    /// A global name read as the lane takes it (`dispatch_fast::lane_load_global_name_refresh`):
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
        if let Some(object) = self.heap.object_record(value) {
            return self.shape_answers(object, atom, ReceiverKind::Ordinary);
        }
        if let Some(object) = self.heap.array(value) {
            if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX) {
                return None;
            }
            return self.shape_answers(object, atom, ReceiverKind::Exotic);
        }
        if let Some(function) = self.heap.function(value) {
            return self.shape_property_lookup_function(function, atom);
        }
        if let Some(iterator) = self.heap.iterator(value) {
            return self.shape_answers(&iterator.object, atom, ReceiverKind::Exotic);
        }
        // A promise's internal slots change no object operation, as for its cell form below.
        if let Some(promise) = self.heap.promise(value) {
            return self.shape_answers(&promise.object, atom, ReceiverKind::Ordinary);
        }
        self.shape_property_lookup_cell(self.heap.get(value)?, atom)
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
    ) -> Option<&'a Object> {
        if atom == self.length_atom || self.atom_class(atom).contains(AtomClass::RESTRICTED_FUNCTION_PROPERTY) {
            return None;
        }
        self.shape_answers(&function.object, atom, ReceiverKind::Ordinary)
    }

    pub(super) fn shape_property_lookup_cell<'a>(
        &self,
        cell: &'a Cell,
        atom: Atom,
    ) -> Option<&'a Object> {
        let class = self.atom_class(atom);
        let kind = match cell {
            Cell::Object(_) | Cell::Promise(_) => ReceiverKind::Ordinary,
            Cell::Proxy { .. } => return None,
            Cell::Array { .. } if class.contains(AtomClass::ARRAY_INDEX) => return None,
            Cell::TypedArray { .. } if class.contains(AtomClass::TYPED_ARRAY_INDEX) => {
                return None;
            }
            Cell::Function(function) => return self.shape_property_lookup_function(function, atom),
            _ => ReceiverKind::Exotic,
        };
        self.shape_answers(cell.object()?, atom, kind)
    }

    /// `object`'s shape answers `atom` unless a private name, a module namespace, a dictionary
    /// shape or, for an exotic receiver, a virtual field intervenes. Only exotic receivers
    /// answer `length`, `size` and the buffer views' fields outside their shape; an ordinary
    /// object stores them like any other property.
    fn shape_answers<'a>(
        &self,
        object: &'a Object,
        atom: Atom,
        kind: ReceiverKind,
    ) -> Option<&'a Object> {
        let excluded = match kind {
            ReceiverKind::Ordinary => AtomClass::PRIVATE,
            ReceiverKind::Exotic => AtomClass::PRIVATE | AtomClass::VIRTUAL_FIELD,
        };
        if self.atom_class(atom).intersects(excluded)
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

