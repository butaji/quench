use super::property_key::PropertyKey;
use super::*;
use crate::heap::PrivateBrand;

/// Where constructors of one shape hold their own `@@species` getter
/// (`Vm::has_intrinsic_species_getter`) under one `prototype_layout_epoch`: the shape names
/// the slot and that the slot holds the getter itself, and every collection advances the
/// epoch, so a proof never names a reused shape. The getter may change in place, so each use
/// reads the slot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct SpeciesSlotProof {
    shape: u32,
    epoch: u64,
    slot: usize,
}

pub(super) enum DiagnosticProperty {
    Absent,
    Data(Value),
    Blocked,
}

impl<H: Host> Vm<H> {
    /// `constructor[@@species]`. Every intrinsic `get [Symbol.species]` returns its receiver
    /// and observes nothing, so an own accessor that still holds one answers without a call
    /// (`has_intrinsic_species_getter`).
    pub(super) fn species_of(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
    ) -> Result<Value, JsError> {
        let Some(species) = self.well_known_symbols.get(WellKnownSymbol::Species) else {
            return Ok(Value::UNDEFINED);
        };
        if self.has_intrinsic_species_getter(constructor) {
            Ok(constructor)
        } else {
            self.get_index(p, constructor, species)
        }
    }

    /// Whether `constructor`'s own `@@species` is an accessor whose getter is an intrinsic
    /// `get [Symbol.species]`, which returns its receiver and observes nothing. A proof for
    /// the constructor's shape (`SpeciesSlotProof`) names the slot holding the getter, so a
    /// repeated check reads that slot alone.
    pub(super) fn has_intrinsic_species_getter(&self, constructor: Value) -> bool {
        let Some(data) = self.object_data(constructor) else {
            return false;
        };
        let (shape, epoch) = (data.shape(), crate::heap::prototype_layout_epoch());
        let getter = match self.species_slot_proof.get() {
            Some(proof) if proof.shape == shape && proof.epoch == epoch => {
                self.heap.property_get(data, proof.slot)
            }
            _ => self.prove_species_slot(constructor, shape, epoch),
        };
        getter.is_some_and(|getter| {
            matches!(
                self.heap.function(getter),
                Some(FunctionRecord {
                    kind: FunctionKind::Native(native),
                    ..
                }) if native.is_intrinsic_species_getter()
            )
        })
    }

    /// `has_intrinsic_species_getter` past its proof: the getter of `constructor`'s own
    /// `@@species` accessor, recording its slot when the shape alone says the slot holds the
    /// getter (`ShapeAttributes::slot_getter` on a non-dictionary shape).
    #[cold]
    #[inline(never)]
    fn prove_species_slot(&self, constructor: Value, shape: u32, epoch: u64) -> Option<Value> {
        let key = PropertyKey::symbol(self.well_known_symbols.get(WellKnownSymbol::Species)?);
        if let Some(slot) = self.property_shape_slot(shape, key)
            && !self.shape_is_dictionary(shape)
            && self.shape_slot_attributes(shape, slot).is_some_and(ShapeAttributes::slot_getter)
        {
            self.species_slot_proof.set(Some(SpeciesSlotProof { shape, epoch, slot }));
        }
        self.property_attributes(constructor, key)
            .and_then(|attributes| attributes.getter.filter(|_| attributes.accessor))
    }

    pub(super) fn diagnostic_own_property(&self, object: Value, key: PropertyKey) -> DiagnosticProperty {
        if self.default_function_name_info(object, key)
            || self.proxy_parts(object).is_some()
            || self.property_attributes(object, key).is_some_and(|attributes| attributes.accessor)
        {
            return DiagnosticProperty::Blocked;
        }
        let value = match key {
            PropertyKey::String(atom) => self.own_property(object, atom),
            PropertyKey::Symbol(symbol) => self.symbol_property(object, symbol),
            PropertyKey::Private(_) => return DiagnosticProperty::Blocked,
        };
        value.map_or(DiagnosticProperty::Absent, DiagnosticProperty::Data)
    }

    /// Inspect data properties for diagnostics without invoking accessors or Proxy traps.
    pub(super) fn diagnostic_data_property(&self, mut object: Value, key: PropertyKey) -> Option<Value> {
        loop {
            let data = self.object_data(object)?;
            match self.diagnostic_own_property(object, key) {
                DiagnosticProperty::Data(value) => return Some(value),
                DiagnosticProperty::Blocked => return None,
                DiagnosticProperty::Absent => {}
            }
            object = data.proto();
            if object.is_null() { return None; }
        }
    }

    pub(super) fn nullish_property_error(&mut self, p: &ResidualProgram, object: Value, property: Option<&str>) -> JsError {
        let kind = if object.is_null() { "null" } else { "undefined" };
        let message = match property { Some(property) => format!("Cannot read properties of {kind} (reading '{property}')"), None => format!("Cannot read properties of {kind}") };
        self.type_error(p, message)
    }

    pub(super) fn module_binding_value(&self, object: Value, atom: Atom) -> Option<Value> {
        let (program, slot) = self.object_data(object)?.module_binding(atom)?;
        if let Some(value) = self.module_import_value(program, super::ROOT_FUNCTION_ID, usize::from(slot)) {
            return Some(value);
        }
        let environment = self.programs.module_environment(program)?;
        self.heap.environment_slot(environment, slot as usize)
    }

    pub(super) fn module_namespace_value(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
    ) -> Result<Option<Value>, JsError> {
        let value = self
            .module_binding_value(object, atom)
            .or_else(|| self.own_property(object, atom));
        let value = match value {
            Some(value) => Some(self.resolve_pending_binding(p, value)?),
            None => None,
        };
        if value.is_some_and(Value::is_deleted) {
            return Err(self.uninitialized_binding_error(p, atom));
        }
        Ok(value)
    }

    fn proxy_trap(
        &mut self,
        p: &ResidualProgram,
        handler: Value,
        name: &str,
    ) -> Result<Value, JsError> {
        let atom = self.intern_atom(name);
        self.get_property(p, handler, atom)
    }

    fn proxy_key_value(
        &mut self,
        p: &ResidualProgram,
        property: PropertyKey,
    ) -> Result<Value, JsError> {
        match property {
            PropertyKey::String(atom) if !self.is_private_name(atom) => {
                Ok(self.atom_string(atom))
            }
            PropertyKey::Symbol(key) => Ok(key),
            PropertyKey::String(_) | PropertyKey::Private(_) => {
                Err(self.type_error(p, "private member is not present on this object".into()))
            }
        }
    }

    pub(super) fn proxy_get(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        property: PropertyKey,
    ) -> Result<Value, JsError> {
        let key = self.proxy_key_value(p, property)?;
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        let target = self.heap.root(target);
        let handler = self.heap.root(handler);
        let receiver = self.heap.root(receiver);
        let key = self.heap.root(key);
        let mut result_root = None;
        let outcome = (|| {
            let trap = self.proxy_trap(p, self.heap.root_value(handler).unwrap(), "get")?;
            if trap.is_null() || trap.is_undefined() {
                let target = self.heap.root_value(target).unwrap();
                let receiver = self.heap.root_value(receiver).unwrap();
                return match property {
                    PropertyKey::String(atom) => {
                        self.get_property_with_receiver(p, target, atom, receiver)
                    }
                    PropertyKey::Symbol(_) => self.get_symbol_property_with_receiver(
                        p,
                        target,
                        self.heap.root_value(key).unwrap(),
                        receiver,
                    ),
                    PropertyKey::Private(_) => unreachable!("private keys cannot reach Proxy Get"),
                };
            }
            if !self.is_function(trap) {
                return Err(self.type_error(p, "proxy get trap is not callable".into()));
            }
            let result = self.call_value(
                p,
                trap,
                self.heap.root_value(handler).unwrap(),
                &[
                    self.heap.root_value(target).unwrap(),
                    self.heap.root_value(key).unwrap(),
                    self.heap.root_value(receiver).unwrap(),
                ],
            )?;
            let result = self.heap.root(result);
            result_root = Some(result);
            self.validate_proxy_get(p, target, key, result)?;
            Ok(self.heap.root_value(result).unwrap())
        })();
        for root in [
            Some(target),
            Some(handler),
            Some(receiver),
            Some(key),
            result_root,
        ]
        .into_iter()
        .flatten()
        {
            self.heap.release_root(root);
        }
        outcome
    }

    fn validate_proxy_get(
        &mut self,
        p: &ResidualProgram,
        target: RootId,
        key: RootId,
        result: RootId,
    ) -> Result<(), JsError> {
        let Some(descriptor) = self.own_property_descriptor_record(
            p,
            self.heap.root_value(target).unwrap(),
            self.heap.root_value(key).unwrap(),
        )?
        else {
            return Ok(());
        };
        if descriptor.configurable == Some(true) {
            return Ok(());
        }
        let result = self.heap.root_value(result).unwrap();
        if descriptor.has_data_fields()
            && descriptor.writable == Some(false)
            && !self.same_value(descriptor.value.unwrap_or(Value::UNDEFINED), result)
        {
            return Err(self.type_error(
                p,
                "proxy get trap returned a different value for a frozen property".into(),
            ));
        }
        if descriptor.has_accessor_fields()
            && descriptor.getter.is_none_or(Value::is_undefined)
            && !result.is_undefined()
        {
            return Err(self.type_error(
                p,
                "proxy get trap returned a value for an accessor without a getter".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn proxy_set(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        property: PropertyKey,
        value: Value,
    ) -> Result<bool, JsError> {
        self.proxy_set_mode(p, target, handler, receiver, property, value, false)
    }

    pub(super) fn proxy_set_mode(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        property: PropertyKey,
        value: Value,
        strict: bool,
    ) -> Result<bool, JsError> {
        let key = self.proxy_key_value(p, property)?;
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        let target = self.heap.root(target);
        let handler = self.heap.root(handler);
        let receiver = self.heap.root(receiver);
        let key = self.heap.root(key);
        let value = self.heap.root(value);
        let outcome = (|| {
            let trap = self.proxy_trap(p, self.heap.root_value(handler).unwrap(), "set")?;
            if trap.is_null() || trap.is_undefined() {
                let delegate_target = self.heap.root_value(target).unwrap();
                let receiver = self.heap.root_value(receiver).unwrap();
                let value = self.heap.root_value(value).unwrap();
                if let Some((inner, handler)) = self.proxy_parts(delegate_target) {
                    return self.proxy_set_mode(p, inner, handler, receiver, property, value, strict);
                }
                return match property {
                    PropertyKey::String(atom) => {
                        let written = self.set_property_with_receiver(p, delegate_target, atom, value, receiver)?;
                        if !written && strict {
                            return Err(self.rejected_assignment_error(p, self.heap.root_value(target).unwrap(), atom, true));
                        }
                        Ok(written)
                    }
                    PropertyKey::Symbol(_) => self.set_symbol_property_with_receiver(
                        p,
                        delegate_target,
                        self.heap.root_value(key).unwrap(),
                        value,
                        receiver,
                    ),
                    PropertyKey::Private(_) => unreachable!("private keys cannot reach Proxy Set"),
                };
            }
            if !self.is_function(trap) {
                return Err(self.type_error(p, "proxy set trap is not callable".into()));
            }
            let result = self.call_value(
                p,
                trap,
                self.heap.root_value(handler).unwrap(),
                &[
                    self.heap.root_value(target).unwrap(),
                    self.heap.root_value(key).unwrap(),
                    self.heap.root_value(value).unwrap(),
                    self.heap.root_value(receiver).unwrap(),
                ],
            )?;
            if !self.truthy(result) {
                if strict {
                    let property = self.detail_string(self.heap.root_value(key).unwrap());
                    return Err(self.type_error(p, format!("'set' on proxy: trap returned falsish for property '{property}'")));
                }
                return Ok(false);
            }
            let descriptor = self.own_property_descriptor_record(
                p,
                self.heap.root_value(target).unwrap(),
                self.heap.root_value(key).unwrap(),
            )?;
            if let Some(descriptor) = descriptor
                && descriptor.configurable != Some(true)
            {
                let is_data = descriptor.has_data_fields();
                if is_data
                    && descriptor.writable != Some(true)
                    && descriptor.value.is_some_and(|target_value| {
                        !self.same_value(self.heap.root_value(value).unwrap(), target_value)
                    })
                {
                    return Err(self
                        .type_error(p, "proxy set trap changed a frozen target property".into()));
                }
                if !is_data && descriptor.setter.is_none_or(Value::is_undefined) {
                    return Err(self.type_error(
                        p,
                        "proxy set trap accepted a property without a setter".into(),
                    ));
                }
            }
            Ok(true)
        })();
        for root in [target, handler, receiver, key, value] {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn get_symbol_property_with_receiver(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if self.is_host_private_symbol(key) {
            if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) { return Ok(Value::UNDEFINED); }
            let Some((value, attributes)) = self.ordinary_own_property(p, object, key)? else { return Ok(Value::UNDEFINED); };
            return if attributes.accessor {
                match attributes.getter.filter(|value| !value.is_undefined()) {
                    Some(getter) => self.call_value(p, getter, receiver, &[]),
                    None => Ok(Value::UNDEFINED),
                }
            } else { Ok(value) };
        }
        if object == receiver
            && let Some(value) = self.plain_property_get(object, PropertyKey::symbol(key))
        {
            return Ok(value);
        }
        let mut owner = self.primitive_prototype(object).unwrap_or(object);
        loop {
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = self.heap.get(owner)
            {
                let (target, handler) = (*target, *handler);
                return self.proxy_get(p, target, handler, receiver, PropertyKey::symbol(key));
            }
            if let Some(value) = self.symbol_property(owner, key) {
                let attributes = self
                    .property_attributes(owner, PropertyKey::symbol(key))
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                if attributes.accessor {
                    return attributes
                        .getter
                        .filter(|getter| !getter.is_undefined())
                        .map_or(Ok(Value::UNDEFINED), |getter| {
                            self.call_value(p, getter, receiver, &[])
                        });
                }
                return Ok(value);
            }
            let Some(data) = self.object_data(owner) else {
                return Ok(Value::UNDEFINED);
            };
            owner = data.proto();
            if owner.is_null() {
                return Ok(Value::UNDEFINED);
            }
        }
    }

    pub(super) fn get_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        self.get_property_with_receiver(p, object, atom, object)
    }

/// `[[Get]]` of a string key along a chain of ordinary objects, decided by shape facts
    /// alone: the nearest data property's value, or `undefined` past the chain's end. `None`
    /// leaves accessors (lazy properties among them), exotic and namespace objects, Proxies
    /// and private, index, `length` or legacy function keys to the general path.
    #[inline]
    pub(super) fn plain_named_get(&self, object: Value, atom: Atom) -> Option<Value> {
        if !self.plain_named_key(atom) {
            return None;
        }
        self.plain_property_get(object, PropertyKey::string(atom))
    }

    /// A global object binding's lookup where it runs no guest code: a present data
    /// property of the global object or its ordinary prototypes, or `Some(None)` for an
    /// absent name (`plain_property_lookup`). `None` for any lookup the shapes alone do not
    /// answer.
    pub(super) fn plain_global_lookup(&self, atom: Atom) -> Option<Option<Value>> {
        if !self.plain_named_key(atom) {
            return None;
        }
        self.plain_property_lookup(self.realm.globals, PropertyKey::string(atom))
    }

    /// Whether `atom` names a property that ordinary shapes alone decide: no private, index,
    /// `length` or legacy function key.
    #[inline]
    pub(super) fn plain_named_key(&self, atom: Atom) -> bool {
        const GENERAL_KEYS: u8 = AtomClass::PRIVATE
            | AtomClass::ARRAY_INDEX
            | AtomClass::TYPED_ARRAY_INDEX
            | AtomClass::RESTRICTED_FUNCTION_PROPERTY;
        atom != self.length_atom && !self.atom_class(atom).intersects(GENERAL_KEYS)
    }

    /// `[[Set]]` of a string key on its own receiver along a chain of extensible plain
    /// objects, decided by the shape facts `plain_named_get` reads: the nearest data
    /// property decides, and an absent key is added. `None` leaves accessors, read-only
    /// properties, side-table descriptors, exotic, namespace and non-extensible objects,
    /// Proxies and the keys `plain_named_key` refuses to the general path, which runs no
    /// guest code before reaching one.
    pub(super) fn set_plain_named_property(
        &mut self,
        object: Value,
        atom: Atom,
        value: Value,
    ) -> Option<()> {
        if !self.plain_named_key(atom) {
            return None;
        }
        self.set_plain_key_property(object, atom, value)
    }

    /// `[[Get]]` of array index `index` along a chain of ordinary objects, which hold an index
    /// key as any other string key: its atom, found without spelling it
    /// (`lookup_array_index_atom`), read as `plain_property_lookup` reads a name. `None` for a
    /// chain through an exotic or arguments object, whose indices live outside the shape, an
    /// index no atom spells yet, or what that walk declines.
    pub(super) fn plain_index_get(&self, object: Value, index: usize) -> Option<Value> {
        if !self.ordinary_index_chain(object) {
            return None;
        }
        self.plain_property_get(object, PropertyKey::string(self.lookup_array_index_atom(index)?))
    }

    /// `set_plain_named_property` of array index `index` on a chain `plain_index_get` reads.
    pub(super) fn set_plain_index_property(&mut self, object: Value, index: usize, value: Value) -> Option<()> {
        if !self.ordinary_index_chain(object) {
            return None;
        }
        let atom = self.lookup_array_index_atom(index)?;
        self.set_plain_key_property(object, atom, value)
    }

    /// Whether every object from `object` up its prototype chain is ordinary and no arguments
    /// object, so an index key is a string key of its shape like any other.
    fn ordinary_index_chain(&self, object: Value) -> bool {
        let mut current = object;
        loop {
            let Some(data) = self.heap.ordinary_object(current) else {
                return false;
            };
            if data.is_arguments_object() {
                return false;
            }
            current = data.proto();
            if current.is_null() {
                return true;
            }
        }
    }

    /// The chain walk and store of `set_plain_named_property` for a key the caller admitted.
    fn set_plain_key_property(&mut self, object: Value, atom: Atom, value: Value) -> Option<()> {
        // A global object's bindings carry declaration state beside its shape.
        if self.is_global_object(object) {
            return None;
        }
        let key = PropertyKey::string(atom);
        let mut current = object;
        loop {
            let data = self.heap.ordinary_object(current)?;
            if !data.is_extensible()
                || data.is_module_namespace()
                || data.deferred_module().is_some()
                || (data.has_side_descriptors() && self.descriptors.contains_key(&(current, key)))
            {
                return None;
            }
            let (shape, proto) = (data.shape(), data.proto());
            if let Some(slot) = self.property_shape_slot(shape, key) {
                let attributes = self.slot_attributes(data, slot)?;
                if attributes.accessor || !attributes.writable {
                    return None;
                }
                break;
            }
            if proto.is_null() {
                break;
            }
            current = proto;
        }
        self.set_shape_property(object, key, value).ok()
    }

    /// A keyed `[[Set]]` of a string naming an existing atom, made as
    /// `set_plain_named_property` makes the named one; `false` leaves the write, and any key
    /// that function refuses, to the general path, which interns a new name.
    pub(super) fn try_plain_string_key_set(&mut self, object: Value, key: Value, value: Value) -> bool {
        let Some(atom) = self.lookup_string_atom(key) else {
            return false;
        };
        if !self.plain_named_key(atom) {
            return false;
        }
        // An add a keyed store made before replays from its entry (`keyed_add_hit`).
        if self.keyed_add_hit(object, atom, value) {
            return true;
        }
        let guard = self.keyed_add_guard(object, atom);
        if self.set_plain_key_property(object, atom, value).is_none() {
            return false;
        }
        if let Some((shape, guard)) = guard {
            self.record_keyed_add(object, atom, shape, guard);
        }
        true
    }

    /// A keyed read of a symbol, or of a string naming an existing non-index atom, that the
    /// plain chain walk answers: what `get_index` returns for it before any lookup that can
    /// run guest code. `None` leaves the read to `get_index`.
    pub(super) fn plain_keyed_get(&mut self, object: Value, key: Value) -> Option<Value> {
        if let Some(index) = key.as_int() {
            return self.plain_index_get(object, usize::try_from(index).ok()?);
        }
        if let Some(atom) = self.lookup_string_atom(key) {
            return self.plain_named_get(object, atom);
        }
        if self.heap.string(key).is_some() {
            return None;
        }
        match self.heap.get(key)? {
            // A host-private symbol names only an own property, which the general path reads.
            Cell::Symbol(symbol) if symbol.kind != crate::heap::SymbolKind::HostPrivate => {
                match self.heap.array(object) {
                    // No proof records an array's shape; the walk reads it as ordinary.
                    Some(_) => self.plain_property_get(object, PropertyKey::symbol(key)),
                    None => self.plain_proved_get(object, PropertyKey::symbol(key)),
                }
            }
            _ => None,
        }
    }

    /// A symbol-keyed read at lane site `site` (a record's address) of the own data slot its
    /// site entry names for `object`'s shape (`SymbolLoadProofs::site_own_slot`).
    #[inline(always)]
    pub(super) fn site_symbol_own_read(&self, site: usize, object: Value, key: Value) -> Option<Value> {
        let data = self.heap.ordinary_object(object)?;
        let epoch = crate::heap::prototype_layout_epoch();
        let slot = self.symbol_load_proofs.site_own_slot(site, data.shape(), key, epoch)?;
        // SAFETY: the entry copies a proof's own data slot of this shape.
        Some(unsafe { self.heap.property_get_unchecked(data, slot as usize) })
    }

    /// Records at lane site `site` the own slot a proof gives `key` on `object`'s shape, for
    /// `site_symbol_own_read`; nothing for any other key or answer.
    pub(super) fn record_site_symbol_read(&self, site: usize, object: Value, key: Value) {
        if !matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            return;
        }
        if let Some(data) = self.heap.ordinary_object(object) {
            let epoch = crate::heap::prototype_layout_epoch();
            self.symbol_load_proofs.record_site_own_slot(site, data.shape(), key, epoch);
        }
    }

    /// `plain_named_get` with its walk proved (`plain_proved_get`), for callers that may record.
    #[inline]
    pub(super) fn plain_named_load(&mut self, object: Value, atom: Atom) -> Option<Value> {
        if !self.plain_named_key(atom) {
            return None;
        }
        self.plain_proved_get(object, PropertyKey::string(atom))
    }

    /// `plain_property_get` of a symbol key or a key `plain_named_key` admits, answered by
    /// the proof of where the walk from a receiver of the same shape found it
    /// (`symbol_load_proofs`), or by `prove_symbol_load`.
    #[inline]
    fn plain_proved_get(&mut self, object: Value, key: PropertyKey) -> Option<Value> {
        use super::symbol_load_proof::SymbolLoadAnswer;
        let data = self.heap.ordinary_object(object)?;
        let epoch = crate::heap::prototype_layout_epoch();
        let answer = self.symbol_load_proofs.answer(data.shape(), key, epoch);
        // A proof's shape is an ordinary one, so the receiver is no namespace, deferred module
        // or arguments object; side descriptors could name the key outside its shape.
        let inherits = !data.has_side_descriptors();
        match answer {
            // SAFETY: the proof recorded this slot for an own data property of this shape.
            Some(SymbolLoadAnswer::Own(slot)) => {
                return Some(unsafe { self.heap.property_get_unchecked(data, slot as usize) });
            }
            Some(SymbolLoadAnswer::Inherited { prototype, holder, slot })
                if inherits && data.proto() == prototype =>
            {
                let holder = self.heap.ordinary_object(holder)?;
                // SAFETY: the proof keeps the holder and the shape its slot was recorded in.
                return unsafe { self.heap.property_get_shaped(holder, slot as usize) };
            }
            Some(SymbolLoadAnswer::Absent { prototype }) if inherits && data.proto() == prototype => {
                return Some(Value::UNDEFINED);
            }
            Some(SymbolLoadAnswer::Getter { prototype, .. }) if inherits && data.proto() == prototype => {
                return None;
            }
            _ => {}
        }
        self.prove_symbol_load(object, key, epoch)
    }

    /// The getter a symbol-keyed read of `key` on `object` calls, where a proof records it
    /// (`SymbolLoadAnswer::Getter`).
    pub(super) fn proved_symbol_getter(&self, object: Value, key: Value) -> Option<Value> {
        use super::symbol_load_proof::SymbolLoadAnswer;
        let data = self.heap.ordinary_object(object)?;
        let epoch = crate::heap::prototype_layout_epoch();
        match self.symbol_load_proofs.answer(data.shape(), PropertyKey::symbol(key), epoch)? {
            SymbolLoadAnswer::Getter { prototype, holder, slot }
                if !data.has_side_descriptors() && data.proto() == prototype =>
            {
                self.slot_attributes(self.heap.ordinary_object(holder)?, slot as usize)?
                    .getter
            }
            _ => None,
        }
    }

    /// `plain_symbol_get` past its proofs: the chain walk, recording where it found the key
    /// when every object it passed is an ordinary, non-dictionary record whose shape alone
    /// says what it holds. Every prototype passed becomes a prototype first, so a later change
    /// to one advances the epoch and retires the proof.
    #[cold]
    #[inline(never)]
    fn prove_symbol_load(&mut self, object: Value, key: PropertyKey, epoch: u64) -> Option<Value> {
        use super::symbol_load_proof::SymbolLoadAnswer;
        let symbol = key;
        let data = self.heap.ordinary_object(object)?;
        let (shape, prototype) = (data.shape(), data.proto());
        let mut current = object;
        let mut depth = 0u8;
        let (answer, value) = loop {
            let link = if depth == 0 {
                self.heap.ordinary_object(current)
            } else {
                self.heap.object_record(current)
            };
            let Some(link) = link else {
                return self.plain_property_get(object, symbol);
            };
            let link_shape = link.shape();
            if link.is_module_namespace()
                || link.deferred_module().is_some()
                || link.is_arguments_object()
                || self.shape_is_dictionary(link_shape)
            {
                return self.plain_property_get(object, symbol);
            }
            if let Some(slot) = self.property_shape_slot(link_shape, symbol) {
                let accessors = self.shape_may_have_accessors(link_shape);
                if accessors && self.slot_attributes(link, slot)?.accessor {
                    if depth > 0 && let Ok(slot) = u32::try_from(slot) {
                        let getter = SymbolLoadAnswer::Getter { prototype, holder: current, slot };
                        self.prove_symbol_chain(shape, key, epoch, prototype, depth, getter);
                    }
                    return None;
                }
                let value = self.heap.property_get(link, slot)?;
                let slot = u32::try_from(slot).ok()?;
                // An own slot's kind must follow from the shape the proof names; a holder's
                // follows from the holder, which the proof names.
                if depth == 0 && accessors {
                    return Some(value);
                }
                let answer = if depth == 0 {
                    SymbolLoadAnswer::Own(slot)
                } else {
                    SymbolLoadAnswer::Inherited { prototype, holder: current, slot }
                };
                break (answer, value);
            }
            if link.has_side_descriptors() {
                return self.plain_property_get(object, symbol);
            }
            let next = link.proto();
            if next.is_null() {
                break (SymbolLoadAnswer::Absent { prototype }, Value::UNDEFINED);
            }
            let Some(deeper) = depth.checked_add(1) else {
                return self.plain_property_get(object, symbol);
            };
            depth = deeper;
            current = next;
        };
        self.prove_symbol_chain(shape, key, epoch, prototype, depth, answer);
        Some(value)
    }

    /// Records `answer` for `key` on receivers of `shape` once the `depth` prototypes from
    /// `prototype` the walk passed are marked prototypes.
    fn prove_symbol_chain(
        &mut self,
        shape: u32,
        key: PropertyKey,
        epoch: u64,
        prototype: Value,
        depth: u8,
        answer: super::symbol_load_proof::SymbolLoadAnswer,
    ) {
        self.mark_prototype_links(prototype, depth);
        // Marking a link that was no prototype yet leaves the epoch where it was.
        if crate::heap::prototype_layout_epoch() == epoch {
            self.symbol_load_proofs.record(shape, key, epoch, answer);
        }
    }

    /// `plain_named_get`'s chain walk for a key it admits, or any symbol key: symbols name
    /// no exotic, index or virtual property.
    #[inline]
    fn plain_property_get(&self, object: Value, key: PropertyKey) -> Option<Value> {
        self.plain_property_lookup(object, key)
            .map(|found| found.unwrap_or(Value::UNDEFINED))
    }

    /// The chain walk of `plain_property_get`, telling an absent key (`Some(None)`) from a
    /// present one: `HasProperty` and `Get` together where neither can run guest code.
    /// `None` leaves both to the general path.
    #[inline]
    pub(super) fn plain_property_lookup(
        &self,
        object: Value,
        key: PropertyKey,
    ) -> Option<Option<Value>> {
        // An array is ordinary for a symbol key: only index keys and `length` are exotic.
        let symbol = key.symbol_value().is_some();
        let mut current = object;
        loop {
            let data = match self.heap.ordinary_object(current) {
                Some(data) => data,
                None if symbol && self.heap.array(current).is_some() => self.heap.object(current)?,
                None => return None,
            };
            if data.is_module_namespace() || data.deferred_module().is_some() {
                return None;
            }
            let shape = data.shape();
            if let Some(slot) = self.property_shape_slot(shape, key) {
                if self.shape_may_have_accessors(shape) && self.slot_attributes(data, slot)?.accessor {
                    return None;
                }
                return self.heap.property_get(data, slot).map(Some);
            }
            if data.has_side_descriptors() && self.descriptors.contains_key(&(current, key)) {
                return None;
            }
            let proto = data.proto();
            if proto.is_null() {
                return Some(None);
            }
            current = proto;
        }
    }

    pub(super) fn get_property_with_receiver(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        atom: Atom,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if let Some(value) = self.plain_named_load(object, atom) {
            return Ok(value);
        }
        let _stack = self.enter_stack()?;
        let private_name = self.is_private_name(atom);
        if object.is_null() || object.is_undefined() {
            let property = self.atom_name(atom).to_owned();
            return Err(self.nullish_property_error(p, object, Some(&property)));
        }
        if !private_name
            && self
                .atom_class(atom)
                .contains(AtomClass::RESTRICTED_FUNCTION_PROPERTY)
            && self.heap.is_function_value(object)
            && self.own_property(object, atom).is_none()
            && !self.function_caller_is_restricted(object)
        {
            return match self.atom_name(atom) {
                "caller" => self.function_caller_value(object),
                "arguments" => self.function_arguments(object),
                _ => unreachable!("legacy function reflection property"),
            };
        }
        let private_target = object;
        let mut object = object;
        if object.as_bool().is_some() {
            match self.atom_name(atom) {
                "toString" | "valueOf"
                    if self
                        .primitive_prototype(object)
                        .and_then(|prototype| self.own_property(prototype, atom))
                        .is_none() =>
                {
                    return Ok(self.native_value(if self.atom_name(atom) == "toString" {
                        Native::BooleanToString
                    } else {
                        Native::BooleanValueOf
                    }));
                }
                _ => {
                    object = self
                        .primitive_prototype(object)
                        .unwrap_or(self.object_proto)
                }
            }
        } else if object.as_number().is_some() {
            let prototype = self
                .primitive_prototype(object)
                .unwrap_or(self.object_proto);
            let method = self.own_property(prototype, atom);
            let builtin = if self.lookup_atom("toString") == Some(atom) {
                Some(Native::NumberString)
            } else if atom == self.to_fixed_atom {
                Some(Native::NumberFixed)
            } else if atom == self.to_precision_atom {
                Some(Native::NumberPrecision)
            } else {
                None
            };
            if method.is_none()
                && let Some(builtin) = builtin
            {
                return Ok(self.native_value(builtin));
            }
            object = prototype;
        }
        self.evaluate_deferred_namespace_for_key(
            p,
            object,
            Some(super::property_key::PropertyKey::string(atom)),
        )?;
        if private_name {
            self.check_private_brand(p, private_target, atom)?;
            if let Some(value) = self.private_field_value(p, private_target, atom) { return Ok(value); }
        }
        loop {
            self.evaluate_deferred_namespace_for_key(
                p,
                object,
                Some(super::property_key::PropertyKey::string(atom)),
            )?;
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = self.heap.get(object)
            {
                let (target, handler) = (*target, *handler);
                // Private names are not property keys observable through a
                // Proxy. Forwarding here would incorrectly let the target's
                // hidden storage satisfy a private access on the Proxy.
                if private_name {
                    return Err(
                        self.type_error(p, "private member is not present on this object".into())
                    );
                }
                return self.proxy_get(p, target, handler, receiver, PropertyKey::string(atom));
            }
            if let Some(attributes) = self.property_attributes(object, PropertyKey::string(atom))
                && attributes.accessor
            {
                if private_name && attributes.getter.is_none() {
                    return Err(
                        self.type_error(p, "private accessor does not have a getter".into())
                    );
                }
                return match attributes.getter {
                    Some(getter) => self.call_value(p, getter, receiver, &[]),
                    None => Ok(Value::UNDEFINED),
                };
            }
            if self
                .object_data(object)
                .is_some_and(Object::is_module_namespace)
            {
                return Ok(self
                    .module_namespace_value(p, object, atom)?
                    .unwrap_or(Value::UNDEFINED));
            }
            if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
                && let Some(index) = super::object_static::array_index(self.atom_name(atom))
                && let Some(elements) = self.heap.array_elements(object)
            {
                let value = elements
                    .get(index as usize)
                    .copied()
                    .filter(|value| !value.is_deleted())
                    .or_else(|| self.heap.sparse_get(object, index as usize));
                if let Some(value) = value.filter(|value| !value.is_deleted()) {
                    return Ok(value);
                }
            }
            if matches!(self.heap.get(object), Some(Cell::TypedArray { .. })) {
                match Self::typed_array_index_key(self.atom_name(atom)) {
                    super::object_descriptors::TypedArrayIndexKey::Index(index) => {
                        return Ok(self
                            .typed_array_get(object, index)
                            .unwrap_or(Value::UNDEFINED));
                    }
                    super::object_descriptors::TypedArrayIndexKey::Invalid => {
                        return Ok(Value::UNDEFINED);
                    }
                    super::object_descriptors::TypedArrayIndexKey::NotCanonical => {}
                }
            }
            if let Some(v) = self.own_property(object, atom) {
                return Ok(v);
            }
            if let Some(data) = self.heap.object_record(object) {
                object = data.proto();
            } else if object.direct_array_index().is_some() {
                object = self
                    .heap
                    .object(object)
                    .expect("live direct array retains its object header")
                    .proto();
            } else {
                match self.heap.get(object) {
                    Some(Cell::ArrayBuffer { object: x, .. }) => object = x.proto(),
                    Some(Cell::TypedArray { object: x, .. }) => object = x.proto(),
                    Some(Cell::DataView { object: x, .. }) => object = x.proto(),
                    _ if let Some(v) = self.heap.string(object)  => {
                        let length = v.len();
                        if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
                            && let Some(index) = super::object_static::array_index(self.atom_name(atom))
                            && let Some(unit) = self.string_index_value(object, index as usize)
                        {
                            return Ok(unit);
                        }
                        if atom == self.length_atom {
                            return Ok(Value::number(length as f64));
                        }
                        let prototype = self
                            .primitive_prototype(object)
                            .unwrap_or(self.string_proto);
                        return self.get_property_with_receiver(p, prototype, atom, object);
                    }
                    Some(Cell::Symbol(description)) => {
                        let description = description.clone();
                        match self.atom_name(atom) {
                            "description" => {
                                return Ok(description
                                    .as_ref()
                                    .map(|value| {
                                        self.heap.alloc_string(value.as_ref().into())
                                    })
                                    .unwrap_or(Value::UNDEFINED));
                            }
                            "toString" => return Ok(self.native_value(Native::SymbolToString)),
                            "valueOf" => return Ok(self.native_value(Native::SymbolValueOf)),
                            _ => {
                                object = self
                                    .primitive_prototype(object)
                                    .unwrap_or(self.object_proto)
                            }
                        }
                    }
                    Some(Cell::BigInt(_)) => {
                        object = self
                            .primitive_prototype(object)
                            .unwrap_or(self.object_proto)
                    }
                    Some(Cell::Date { object: x, .. })
                    | Some(Cell::TemporalDuration { object: x, .. })
                    | Some(Cell::TemporalPlainDate { object: x, .. })
                    | Some(Cell::TemporalPlainDateTime { object: x, .. })
                    | Some(Cell::TemporalPlainMonthDay { object: x, .. })
                    | Some(Cell::TemporalPlainYearMonth { object: x, .. })
                    | Some(Cell::TemporalZonedDateTime { object: x, .. })
                    | Some(Cell::TemporalInstant { object: x, .. }) => object = x.proto(),
                    Some(Cell::Array { object: x, .. }) => object = x.proto(),
                    Some(Cell::ShadowRealm { object: x, .. }) => object = x.proto(),
                    Some(Cell::RegExp { object: x, .. }) => object = x.proto(),
                    Some(Cell::Map { object: x, .. }) | Some(Cell::Set { object: x, .. }) => {
                        object = x.proto()
                    }
                    Some(Cell::WeakMap { object: x, .. })
                    | Some(Cell::WeakSet { object: x, .. }) => object = x.proto(),
                    Some(Cell::WeakRef { object: x, .. }) => object = x.proto(),
                    Some(Cell::FinalizationRegistry { object: x, .. }) => object = x.proto(),
                    Some(Cell::Iterator(x)) => object = x.object.proto(),
                    Some(Cell::Function(x)) => object = x.object.proto(),
                    _ => match self.heap.object(object) {
                        Some(x) => object = x.proto(),
                        None => return Ok(Value::UNDEFINED),
                    },
                }
            }
            if object.is_null() {
                return if private_name {
                    let home = self.private_brand_home(p, private_target, atom);
                    if let Some(home) = home {
                        if let Some(attributes) = self.property_accessor(home, atom) {
                            if let Some(getter) = attributes.getter {
                                return self.call_value(p, getter, receiver, &[]);
                            }
                            return Err(self
                                .type_error(p, "private accessor does not have a getter".into()));
                        }
                        if let Some(value) = self.own_property(home, atom) {
                            return Ok(value);
                        }
                    }
                    Err(self.type_error(p, "private member is not present on this object".into()))
                } else {
                    Ok(Value::UNDEFINED)
                };
            }
        }
    }

    /// The value of private field `atom` of `target` under the running frame's home for the
    /// name. A target without the field, or whose element is a method or accessor, answers
    /// `None`, leaving the access to the general path, which throws or calls it.
    pub(super) fn private_field_value(&mut self, p: &ResidualProgram, target: Value, atom: Atom) -> Option<Value> {
        let home = self.private_name_home(p, atom)?;
        self.object_data(target)?.private_field(PrivateBrand { home, name: atom })
    }

    /// The private brand of `atom` that function `function` of the running program, whose
    /// frame would capture from `environment`, names when it has no binding sites: the home a
    /// run of it with a frame found (`private_name_home`'s memo), else the initialized home
    /// binding its static name binding reads from `environment` (`direct_capture`'s read), as
    /// an arrow field's closure of each new instance finds its class's. `None` leaves the
    /// access to the general path, which finds the home.
    fn memoized_private_brand(
        &self,
        p: &ResidualProgram,
        environment: Value,
        function: u32,
        atom: Atom,
    ) -> Option<PrivateBrand> {
        let body = p.function_get(function as usize).filter(|body| body.binding_sites.is_empty())?;
        let key = PrivateHomeKey { environment, program: self.active_program, function, name: atom };
        if let Some(home) = self.private_homes.get(&key) {
            return Some(PrivateBrand { home: *home, name: atom });
        }
        let binding = body
            .name_bindings
            .get(atom)
            .filter(|binding| binding.kind == crate::bytecode::LexicalBindingKind::PrivateName)?;
        let crate::bytecode::EvalBindingLocation::Capture { depth, slot } = binding.location else {
            return None;
        };
        let (slots, root) = self.environment_capture_slots(environment, false, depth)?;
        Some(PrivateBrand { home: self.owned_capture(slots, root, slot)?, name: atom })
    }

    /// The private brand of `atom` at lane site `site` of `function`, whose frame captures
    /// from `environment`: the home the site found last for that environment
    /// (`PrivateSiteHomes`), else the running frame's home (`private_name_home`) when `frame`
    /// is set, or the memoized one (`memoized_private_brand`) for a frameless activation.
    /// `None` leaves the access to the general path.
    pub(super) fn site_private_brand(
        &mut self,
        p: &ResidualProgram,
        site: usize,
        environment: Value,
        function: u32,
        frame: bool,
        atom: Atom,
    ) -> Option<PrivateBrand> {
        let epoch = self.heap.collection_epoch();
        if let Some(home) = self.private_site_homes.home(site, environment, epoch) {
            return Some(PrivateBrand { home, name: atom });
        }
        let brand = if frame {
            PrivateBrand { home: self.private_name_home(p, atom)?, name: atom }
        } else {
            self.memoized_private_brand(p, environment, function, atom)?
        };
        if p.function_get(function as usize).is_some_and(|body| body.binding_sites.is_empty()) {
            self.private_site_homes.record(site, environment, epoch, brand.home);
        }
        Some(brand)
    }

    /// The private method or accessor that `object`, which carries `brand`, reads at lane site
    /// `site`: the element the site recorded under the brand's home (`PrivateSiteHomes::element`), else the one `[[Get]]`'s walk of the
    /// object's chain meets, recorded for the next read. `None` leaves the read to the general
    /// path: a missing getter, a proxy on the chain, or a site without a recorded home.
    pub(super) fn site_private_element(
        &self,
        site: usize,
        brand: PrivateBrand,
        object: Value,
    ) -> Option<super::private_site_home::PrivateElement> {
        use super::private_site_home::PrivateElement;
        let epoch = self.heap.collection_epoch();
        if let Some(element) = self.private_site_homes.element(site, epoch, brand.home) {
            return Some(element);
        }
        let key = PropertyKey::string(brand.name);
        let mut link = object;
        let element = loop {
            if matches!(self.heap.get(link), Some(Cell::Proxy { .. })) {
                return None;
            }
            if let Some(attributes) = self.property_attributes(link, key) {
                break if attributes.accessor {
                    PrivateElement::Getter(attributes.getter?)
                } else {
                    PrivateElement::Method(self.own_property(link, brand.name)?)
                };
            }
            link = self.object_data(link)?.proto();
            if link.is_null() {
                return None;
            }
        };
        self.private_site_homes.record_element(site, epoch, brand.home, element);
        Some(element)
    }

    /// `private_field_value` through `memoized_private_brand`.
    pub(super) fn memoized_private_field_value(
        &self,
        p: &ResidualProgram,
        environment: Value,
        function: u32,
        target: Value,
        atom: Atom,
    ) -> Option<Value> {
        let brand = self.memoized_private_brand(p, environment, function, atom)?;
        self.object_data(target)?.private_field(brand)
    }

    /// The store of private field `atom` of `target` under the running frame's home for the
    /// name: `false` for a target without the field, or whose element is a method or
    /// accessor, which the general path throws for or calls.
    pub(super) fn set_private_field_value(&mut self, p: &ResidualProgram, target: Value, atom: Atom, value: Value) -> bool {
        let Some(home) = self.private_name_home(p, atom) else { return false; };
        self.object_data_mut(target).is_some_and(|object| object.set_private_field(PrivateBrand { home, name: atom }, value))
    }

    pub(super) fn initialize_private_field(&mut self, p: &ResidualProgram, target: Value, home: Value, atom: Atom, value: Value) -> Result<(), JsError> {
        let Some(extensible) = self.object_data(target).map(Object::is_extensible) else {
            return Err(self.type_error(p, "Cannot add private field to a non-object".into()));
        };
        if !extensible {
            return Err(self.type_error(p, "Cannot add private field to a non-extensible object".into()));
        }
        if let Some(object) = self.object_data_mut(target)
            && object.initialize_private_field(PrivateBrand { home, name: atom }, value)
        {
            return Ok(());
        }
        let name = crate::bytecode::private_identity_label(self.atom_name(atom)).unwrap_or_else(|| self.atom_name(atom).to_owned());
        Err(self.type_error(p, format!("Cannot initialize #{name} twice on the same object")))
    }

    pub(super) fn check_private_brand(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Result<(), JsError> {
        if self.has_private_brand(p, target, atom) {
            return Ok(());
        }
        Err(self.type_error(p, "private member is not present on this object".into()))
    }

    pub(super) fn has_private_brand(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> bool {
        self.private_brand_home(p, target, atom).is_some()
    }

    /// The home object of `target`'s private name `atom` as seen from the running frame,
    /// if `target` carries it.
    pub(super) fn private_brand_home(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Option<Value> {
        let home = self.private_name_home(p, atom)?;
        self.object_data(target)
            .is_some_and(|object| object.has_private_name(PrivateBrand { home, name: atom }))
            .then_some(home)
    }

    /// The home object that declares private name `atom` for the running frame: the nearest
    /// eval-introduced private binding, otherwise the first visible class home declaring the
    /// name. It depends on the frame's function and environment chain, never on the target,
    /// so a found home is memoized while that environment lives.
    fn private_name_home(&mut self, p: &ResidualProgram, atom: Atom) -> Option<Value> {
        let frame_index = self.frames.len().checked_sub(1)?;
        let start = self.captured_parent_environment(frame_index);
        let key = PrivateHomeKey {
            environment: start,
            program: self.frames[frame_index].program,
            function: self.frames[frame_index].function,
            name: atom,
        };
        // Without binding sites no eval-introduced private binding is in reach, so the name
        // bindings resolve the same initialized home whenever the closure's environment chain
        // is the same.
        let static_bindings = p
            .function_get(key.function as usize)
            .is_some_and(|function| function.binding_sites.is_empty());
        if static_bindings && let Some(home) = self.private_homes.get(&key) {
            return Some(*home);
        }
        if let Some(binding) = self.name_binding(frame_index, atom)
            && binding.kind == crate::bytecode::LexicalBindingKind::PrivateName
        {
            let home = self.load_name_binding(p, frame_index, binding).ok();
            if static_bindings && let Some(home) = home {
                self.private_homes.insert(key, home);
            }
            return home;
        }
        if let Some(home) = self.private_homes.get(&key) {
            return Some(*home);
        }
        let private_home_binding = self.private_home_binding_atom(atom);
        let mut function = Some(self.frames[frame_index].function);
        let mut home_atoms = Vec::new();
        while let Some(id) = function
            && let Some(metadata) = p.function_get(id as usize)
        {
            if let Some(atom) = metadata.super_home_atom
                && !home_atoms.contains(&atom)
            {
                home_atoms.push(atom);
            }
            function = metadata.parent;
        }
        let mut environment = start;
        let mut homes = Vec::with_capacity(home_atoms.len());
        while let Some(Cell::Environment {
            parent,
            function,
            slots,
            scope,
        }) = self.heap.get(environment)
        {
            let program = &scope.program();
            let dynamic_bindings = self.heap.environment_bindings(environment)?;
            if let Some((_, home)) = dynamic_bindings
                .iter()
                .rev()
                .find(|(binding, _)| *binding == private_home_binding)
            {
                return Some(*home);
            }
            if *function != u32::MAX {
                let owner = program.and_then(|program| {
                    self.programs
                        .get(super::program_store::ProgramId::from_raw(program))
                });
                let owner = owner.as_deref().unwrap_or(p);
                let visible_homes = owner
                    .function_get(*function as usize)
                    .into_iter()
                    .flat_map(|metadata| metadata.local_atoms.iter().copied())
                    .filter(|atom| self.atom_name(*atom).starts_with(crate::bytecode::HOME_BINDING_PREFIX))
                    .collect::<Vec<_>>();
                for atom in home_atoms.iter().chain(&visible_homes) {
                    if homes.iter().any(|(candidate, _)| candidate == atom) {
                        continue;
                    }
                    if let Some(slot) = self
                        .local_binding_slot(owner, *function, *atom)
                        .filter(|slot| *slot < slots.len())
                    {
                        homes.push((*atom, self.heap.environment_slot(environment, slot)?));
                    }
                }
            }
            environment = *parent;
        }
        let home = homes.into_iter().map(|(_, home)| home).find(|home| {
            self.object_data(*home).is_some_and(|object| {
                object.has_private_name(PrivateBrand {
                    home: *home,
                    name: atom,
                })
            })
        })?;
        // Eval-introduced private bindings live only in environments made for that eval, so
        // a cached chain never gains a nearer one.
        self.private_homes.insert(key, home);
        Some(home)
    }

    pub(super) fn get_private_proxy_field(
        &mut self,
        p: &ResidualProgram,
        proxy: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        if let Some(value) = self.private_field_value(p, proxy, atom) { return Ok(value); }
        let Some(home) = self.private_brand_home(p, proxy, atom) else {
            return Err(self.type_error(p, "private member is not present on this object".into()));
        };
        if let Some(value) = self.own_property(proxy, atom) {
            return Ok(value);
        }
        if let Some(attributes) = self.property_accessor(home, atom) {
            let Some(getter) = attributes.getter else {
                return Err(
                    self.type_error(p, "private member is not present on this object".into())
                );
            };
            return self.call_value(p, getter, proxy, &[]);
        }
        // Methods live on the home object, which does not carry its own brand.
        self.own_property(home, atom).ok_or_else(|| {
            self.type_error(p, "private member is not present on this object".into())
        })
    }
}
