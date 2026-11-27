use super::property_key::PropertyKey;
use std::ops::ControlFlow;
use super::*;
use crate::heap::PrivateBrand;
pub mod ordinary_chain;
mod function_reflection;
mod plain_key_set;
mod proxy_get;
mod proxy_set;

/// Where constructors of one shape hold their own `@@species` getter
/// (`Vm::has_intrinsic_species_getter`) under one `prototype_layout_epoch`: the shape names
/// the slot and that the slot holds the getter itself, and shape compaction advances the
/// epoch, so a proof never names a renumbered shape. The getter may change in place, so each use
/// reads the slot. It reads no prototype, so it keeps the global epoch, which only the events
/// every proof shares advance (a compaction, a collection reusing a prototype's record), rather
/// than a chain's cell.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct SpeciesSlotProof {
    shape: u32,
    slot: usize,
}

pub(super) enum DiagnosticProperty {
    Absent,
    Data(Value),
    Blocked,
}

impl Vm {
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
        let Some(data) = self.heap.object(constructor) else {
            return false;
        };
        // The proof is of the constructor's own non-dictionary shape alone, which fixes the
        // slot: no prototype is read, so no prototype's change can stale it.
        let shape = data.shape();
        let getter = match self.species_slot_proof.get() {
            Some(proof) if proof.shape == shape => {
                self.heap.property_get_ref(data, proof.slot)
            }
            _ => self.prove_species_slot(constructor, shape),
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
    fn prove_species_slot(&self, constructor: Value, shape: u32) -> Option<Value> {
        let key = PropertyKey::symbol(self.well_known_symbols.get(WellKnownSymbol::Species)?);
        if let Some(slot) = self.property_shape_slot(shape, key)
            && !self.shape_is_dictionary(shape)
            && self.shape_slot_attributes(shape, slot).is_some_and(ShapeAttributes::slot_getter)
        {
            self.species_slot_proof.set(Some(SpeciesSlotProof { shape, slot }));
        }
        self.property_attributes(constructor, key)
            .and_then(|attributes| attributes.getter.filter(|_| attributes.accessor))
    }

    pub(super) fn diagnostic_own_property(&self, object: Value, key: PropertyKey) -> DiagnosticProperty {
        if self.default_function_name_info(object, key)
            || self.proxy_parts(object).is_some()
            || self
                .property_attributes(object, key)
                .is_some_and(|attributes| attributes.accessor)
        {
            return DiagnosticProperty::Blocked;
        }
        let value = match key {
            PropertyKey::String(atom) => self.own_property(object, atom),
            PropertyKey::Symbol(symbol) => self.symbol_property(object, symbol),
            PropertyKey::Private(..) => return DiagnosticProperty::Blocked,
        };
        value.map_or(DiagnosticProperty::Absent, DiagnosticProperty::Data)
    }

    /// Inspect data properties for diagnostics without invoking accessors or Proxy traps.
    pub(super) fn diagnostic_data_property(
        &self,
        mut object: Value,
        key: PropertyKey,
    ) -> Option<Value> {
        loop {
            let data = self.heap.object(object)?;
            match self.diagnostic_own_property(object, key) {
                DiagnosticProperty::Data(value) => return Some(value),
                DiagnosticProperty::Blocked => return None,
                DiagnosticProperty::Absent => {}
            }
            object = data.proto();
            if object.is_null() {
                return None;
            }
        }
    }

    pub(super) fn nullish_property_error(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        property: Option<&str>,
    ) -> JsError {
        let kind = if object.is_null() {
            "null"
        } else {
            "undefined"
        };
        let message = match property {
            Some(property) => format!("Cannot read properties of {kind} (reading '{property}')"),
            None => format!("Cannot read properties of {kind}"),
        };
        self.type_error(p, message)
    }

    pub(super) fn module_binding_value(&self, object: Value, atom: Atom) -> Option<Value> {
        // record-only: module bindings are record state
        let (program, slot) = self.heap.object(object)?.record_only()?.module_binding(atom)?;
        if let Some(value) =
            self.module_import_value(program, super::ROOT_FUNCTION_ID, usize::from(slot))
        {
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
        if self.handler_lacks_trap(handler, ProxyTrap::Set) && !matches!(property, PropertyKey::Private(..)) {
            return self.proxy_set_target(p, target, receiver, property, value, strict);
        }
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
            let trap = self.proxy_trap(p, self.heap.root_value(handler).unwrap(), self.trap_atom(ProxyTrap::Set))?;
            if trap.is_null() || trap.is_undefined() {
                let (delegate_target, receiver, value) = (
                    self.heap.root_value(target).unwrap(),
                    self.heap.root_value(receiver).unwrap(),
                    self.heap.root_value(value).unwrap(),
                );
                return self.proxy_set_target(p, delegate_target, receiver, property, value, strict);
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
            let (target, key, value) = (self.heap.root_value(target).unwrap(), self.heap.root_value(key).unwrap(), self.heap.root_value(value).unwrap());
            let accepted = self.truthy(result);
            self.validate_proxy_store(p, target, key, value, strict, result)?;
            Ok(accepted)
        })();
        for root in [target, handler, receiver, key, value] {
            self.heap.release_root(root);
        }
        outcome
    }

    pub fn get_symbol_property_with_receiver(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if self.is_host_private_symbol(key) {
            if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) {
                return Ok(Value::UNDEFINED);
            }
            let Some((value, attributes)) = self.ordinary_own_property(p, object, key)? else {
                return Ok(Value::UNDEFINED);
            };
            return if attributes.accessor {
                match attributes.getter.filter(|value| !value.is_undefined()) {
                    Some(getter) => self.call_value(p, getter, receiver, &[]),
                    None => Ok(Value::UNDEFINED),
                }
            } else {
                Ok(value)
            };
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
            let Some(data) = self.heap.object(owner) else {
                return Ok(Value::UNDEFINED);
            };
            owner = data.proto();
            if owner.is_null() {
                return Ok(Value::UNDEFINED);
            }
        }
    }

    pub fn get_property(
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
    pub fn plain_named_get(&self, object: Value, atom: Atom) -> Option<Value> {
        if !self.plain_named_key(atom) {
            return None;
        }
        self.plain_property_get(object, PropertyKey::string(atom))
    }

    /// A global object binding's lookup where it runs no guest code: a present data
    /// property of the global object or its ordinary prototypes, or `Some(None)` for an
    /// absent name (`plain_property_lookup`). `None` for any lookup the shapes alone do not
    /// answer.
    pub fn plain_global_lookup(&self, atom: Atom) -> Option<Option<Value>> {
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
    pub fn set_plain_named_property(
        &mut self,
        object: Value,
        atom: Atom,
        value: Value,
    ) -> Option<()> {
        if !self.plain_named_key(atom) {
            return None;
        }
        self.set_plain_key_property(object, atom, value, false)
    }

    /// `[[Get]]` of array index `index` along a chain of ordinary objects, which hold an index
    /// key as any other string key: its atom, found without spelling it
    /// (`lookup_array_index_atom`), read as `plain_property_lookup` reads a name. `None` for a
    /// chain through an exotic or arguments object, whose indices live outside the shape, an
    /// index no atom spells yet, or what that walk declines.
    pub(super) fn plain_index_get(&mut self, object: Value, index: usize) -> Option<Value> {
        if !self.ordinary_index_chain(object) {
            return None;
        }
        self.plain_proved_get(object, PropertyKey::string(self.lookup_array_index_atom(index)?))
    }

    /// `set_plain_named_property` of array index `index` on a chain `plain_index_get` reads.
    pub fn set_plain_index_property(&mut self, object: Value, index: usize, value: Value) -> Option<()> {
        if !self.ordinary_index_chain(object) {
            return None;
        }
        let atom = self.lookup_array_index_atom(index)?;
        self.set_plain_key_property(object, atom, value, false)
    }

    /// Whether every object from `object` up its prototype chain is ordinary and no arguments
    /// object, so an index key is a string key of its shape like any other.
    fn ordinary_index_chain(&self, object: Value) -> bool {
        let mut current = object;
        loop {
            let Some(data) = self.heap.ordinary_object(current) else {
                return false;
            };
            if data.is_arguments_object() || self.string_wrapper_primitive(current).is_some() {
                return false;
            }
            current = data.proto();
            if current.is_null() {
                return true;
            }
        }
    }

    /// A keyed read of a symbol, or of a string naming an existing non-index atom, that the
    /// plain chain walk answers: what `get_index` returns for it before any lookup that can
    /// run guest code. `None` leaves the read to `get_index`.
    pub fn plain_keyed_get(&mut self, object: Value, key: Value) -> Option<Value> {
        if let Some(index) = key.as_int() {
            return self.plain_index_get(object, usize::try_from(index).ok()?);
        }
        if let Some(value) = self.plain_string_keyed_get(object, key) {
            return Some(value);
        }
        if self.heap.string(key).is_some() {
            return None;
        }
        match self.heap.symbol_data(key) {
            // A host-private symbol names only an own property, which the general path reads.
            Some(symbol) if symbol.kind != crate::heap::SymbolKind::HostPrivate => {
                match self.heap.array(object) {
                    // No proof records an array's shape; the walk reads it as ordinary.
                    Some(_) => self.plain_property_get(object, PropertyKey::symbol(key)),
                    None => self.plain_proved_get(object, PropertyKey::symbol(key)),
                }
            }
            _ => None,
        }
    }

    /// A symbol- or index-keyed read at lane site `site` (a record's address) of the own data
    /// slot its site entry names for `object`'s shape (`SymbolLoadProofs::site_own_slot`). An
    /// arguments object's indices are exotic whatever its shape says.
    #[inline(always)]
    pub fn site_keyed_own_read(&self, site: usize, object: Value, key: Value) -> Option<Value> {
        let data = self.heap.ordinary_object(object)?;
        let slot = self.symbol_load_proofs.site_own_slot(site, data.layout_id(), key)?;
        if data.is_arguments_object() {
            return None;
        }
        // SAFETY: the entry copies a proof's own data slot of this shape.
        Some(unsafe { self.heap.property_get_unchecked(data, slot as usize) })
    }

    /// Records at lane site `site` the own slot a proof gives `key`, a symbol or an array
    /// index, on `object`'s shape, for `site_keyed_own_read`; nothing for any other key or
    /// answer.
    pub fn record_site_keyed_read(&self, site: usize, object: Value, key: Value) {
        let property = match key.as_int() {
            Some(index) => match usize::try_from(index).ok().and_then(|index| self.lookup_array_index_atom(index)) {
                Some(atom) => PropertyKey::string(atom),
                None => return,
            },
            None if self.heap.is_symbol(key) => PropertyKey::symbol(key),
            None => return,
        };
        if let Some(data) = self.heap.ordinary_object(object) {
            self.symbol_load_proofs.record_site_own_slot(site, data.layout_id(), key, property);
        }
    }

    /// `plain_named_get` with its walk proved (`plain_proved_get`), for callers that may record.
    #[inline]
    pub fn plain_named_load(&mut self, object: Value, atom: Atom) -> Option<Value> {
        if !self.plain_named_key(atom) {
            return None;
        }
        self.plain_proved_get(object, PropertyKey::string(atom))
    }

    /// The object a proved read of `key` starts from: an ordinary object for any key, and for
    /// a symbol any object but a Proxy, since exotic objects differ only on string keys (a
    /// typed array's, iterator's or collection's `@@iterator`). An array's reads walk unproved
    /// (`plain_keyed_get`).
    #[inline(always)]
    pub(in crate::vm) fn proved_read_receiver(&self, object: Value, key: PropertyKey) -> Option<&Object> {
        if let Some(data) = self.heap.ordinary_object(object) {
            return Some(data);
        }
        // A name no exotic object answers off its shape: a collection, date, regexp, promise or
        // iterator holds every own name on its shape, and an array every one but its indices
        // and `length`, which `plain_named_key` leaves out.
        if let PropertyKey::String(atom) = key
            && self.plain_named_key(atom)
        {
            if let Some(data) = self.heap.ordinary_builtin_object(object) {
                return Some(data);
            }
            if self.heap.array(object).is_some() {
                return self.heap.object(object).and_then(|o| o.record_only());
            }
        }
        if !matches!(key, PropertyKey::Symbol(_))
            || self.heap.array(object).is_some()
            || matches!(self.heap.get(object), Some(Cell::Proxy { .. }))
        {
            return None;
        }
        self.heap.object(object).and_then(|o| o.record_only())
    }

    /// An object `plain_property_lookup` walks through as ordinary for `key`: what a proved
    /// read starts from (`proved_read_receiver`), or an array where `array_ordinary` says
    /// the key is none of its exotic ones.
    #[inline(always)] fn plain_link(&self, object: Value, key: PropertyKey, array_ordinary: bool) -> Option<&Object> {
        self.proved_read_receiver(object, key)
            .or_else(|| (array_ordinary && self.heap.array(object).is_some()).then(|| self.heap.object(object).and_then(|o| o.record_only())).flatten())
    }

    /// `plain_property_get` of a symbol key or a key `plain_named_key` admits, answered by
    /// the proof of where the walk from a receiver of the same shape found it
    /// (`symbol_load_proofs`), or by `prove_symbol_load`.
    #[inline(always)]
    fn plain_proved_get(&mut self, object: Value, key: PropertyKey) -> Option<Value> {
        let Some(data) = self.heap.ordinary_object(object) else { return self.plain_proved_get_past_own(object, key, None) };
        let probed = key.key32().map(|key32| (data.layout_id(), self.symbol_load_proofs.word(data.layout_id(), key32)));
        if let Some(word) = probed.and_then(|(_, word)| word).filter(|word| word.kind() == super::symbol_load_proof::KIND_OWN) {
            // SAFETY: the proof recorded this slot for an own data property of this layout's shape.
            return Some(unsafe { self.heap.property_get_unchecked(data, word.slot()) });
        }
        self.plain_proved_get_past_own(object, key, probed)
    }

    /// `plain_proved_get` past an own-slot hit, given the entry its probe found for the
    /// receiver's layout (`probed`, the layout and the probe's answer, `None` once unprobed):
    /// the other answers, else the proving walk.
    #[inline(never)]
    fn plain_proved_get_past_own(&mut self, object: Value, key: PropertyKey, probed: Option<(u32, Option<super::symbol_load_proof::FactWord>)>) -> Option<Value> {
        use super::symbol_load_proof::{KIND_ABSENT, KIND_GETTER, KIND_GETTER_PAIR, KIND_GETTER_SLOT, KIND_INHERITED, KIND_OWN};
        let data = self.proved_read_receiver(object, key)?;
        if let Some(key32) = key.key32() {
            let word = match probed {
                Some((layout, word)) if layout == data.layout_id() => word,
                _ => self.symbol_load_proofs.word(data.layout_id(), key32),
            };
            if let Some(word) = word {
                // A proof's layout is an ordinary one, so the receiver is no namespace, deferred
                // module or arguments object; side descriptors could name the key outside its shape.
                let inherits = !data.has_side_descriptors();
                match word.kind() {
                    // SAFETY: the proof recorded this slot for an own data property of this shape.
                    KIND_OWN => return Some(unsafe { self.heap.property_get_unchecked(data, word.slot()) }),
                    KIND_INHERITED if inherits => {
                        let holder = match self.symbol_load_proofs.holder(data.layout_id(), key32) {
                            Some(id) if id != 0 => self.heap.object_record_by_id(id - 1),
                            _ => self.proof_holder(data, word.hops())?,
                        };
                        // SAFETY: the entry's subjects keep the holder and the shape its slot was recorded in.
                        return unsafe { self.heap.property_get_shaped(holder, word.slot()) };
                    }
                    KIND_ABSENT if inherits => return Some(Value::UNDEFINED),
                    KIND_GETTER | KIND_GETTER_SLOT | KIND_GETTER_PAIR if inherits || word.hops() == 0 => return None,
                    _ => {}
                }
            }
        }
        self.prove_symbol_load(object, key)
    }

    /// `plain_symbol_get` past its proofs: the chain walk, recording where it found the key
    /// when every object it passed is a non-dictionary object whose shape alone says what it
    /// holds for the key (`proved_read_receiver`). Every prototype passed becomes a prototype first, whose subject holds
    /// the proof: a later change to one clears it.
    #[cold]
    #[inline(never)]
    fn prove_symbol_load(&mut self, object: Value, key: PropertyKey) -> Option<Value> {
        match self.prove_load(object, key) {
            ProvedLoad::Value(value) => Some(value),
            ProvedLoad::Absent => Some(Value::UNDEFINED),
            ProvedLoad::Getter(_) => None,
            ProvedLoad::Unproved => self.plain_property_get(object, key),
        }
    }

    /// The walk of `prove_symbol_load`, recording what it proves: the value, an absent key, a
    /// getter (which only the general path calls), or a chain no proof covers.
    pub(in crate::vm) fn prove_load(&mut self, object: Value, key: PropertyKey) -> ProvedLoad {
        self.prove_load_walk(object, key).unwrap_or(ProvedLoad::Unproved)
    }

    fn prove_load_walk(&mut self, object: Value, key: PropertyKey) -> Option<ProvedLoad> {
        use super::symbol_load_proof::SymbolLoadAnswer;
        let data = self.proved_read_receiver(object, key)?;
        let (layout, prototype) = (data.layout_id(), data.proto());
        let mut current = object;
        let mut depth = 0u8;
        let (answer, value) = loop {
            // Every link, the receiver and each prototype alike, is one whose shape alone
            // answers the key: an ordinary object or builtin, or an array for a name that is
            // no index and not `length` (`Array.prototype` holding `constructor` or a
            // method). A prototype's subject retires the proof on any change to its layout,
            // whatever its kind (`Object::note_prototype_layout_change`).
            let Some(link) = self.proved_read_receiver(current, key) else {
                return Some(ProvedLoad::Unproved);
            };
            let link_shape = link.shape();
            if link.is_module_namespace()
                || link.deferred_module().is_some()
                || link.is_arguments_object()
                || self.shape_is_dictionary(link_shape)
            {
                return Some(ProvedLoad::Unproved);
            }
            if let Some(slot) = self.property_shape_slot(link_shape, key) {
                let accessors = self.shape_may_have_accessors(link_shape);
                let attributes = if accessors { Some(self.slot_attributes(link, slot)?) } else { None };
                if let Some(attributes) = attributes.filter(|attributes| attributes.accessor) {
                    // An own accessor is a fact of the receiver's layout as an own data slot
                    // is (a non-dictionary shape fixes each slot's kind); an inherited one
                    // is held under the prototypes passed.
                    if let Ok(slot) = u32::try_from(slot) {
                        let getter = SymbolLoadAnswer::Getter { hops: depth, slot };
                        // Only an entry the chain's subjects hold is refined.
                        if self.prove_symbol_chain(layout, key, prototype, depth, getter, current) {
                            self.refine_getter_fact(layout, key, depth, slot, current);
                        }
                    }
                    return Some(ProvedLoad::Getter(attributes.getter));
                }
                let value = self.heap.property_get(link, slot)?;
                let slot = u32::try_from(slot).ok()?;
                // A non-dictionary shape fixes each slot's kind, so an own data slot is
                // provable for the shape even where the shape has accessors elsewhere.
                let answer = if depth == 0 {
                    SymbolLoadAnswer::Own(slot)
                } else {
                    SymbolLoadAnswer::Inherited { hops: depth, slot }
                };
                break (answer, value);
            }
            if link.has_side_descriptors() {
                return Some(ProvedLoad::Unproved);
            }
            let next = link.proto();
            if next.is_null() {
                break (SymbolLoadAnswer::Absent, Value::UNDEFINED);
            }
            let Some(deeper) = depth.checked_add(1) else {
                return Some(ProvedLoad::Unproved);
            };
            depth = deeper;
            current = next;
        };
        // The walk's answer stands whether or not it is recorded; an unrecorded one is walked
        // again at its next miss.
        self.prove_symbol_chain(layout, key, prototype, depth, answer, current);
        Some(match answer {
            SymbolLoadAnswer::Absent => ProvedLoad::Absent,
            _ => ProvedLoad::Value(value),
        })
    }

    /// Records `answer` for `key` on receivers of `layout`, held under the subjects of the
    /// `depth` prototypes from `prototype` the walk passed (`Vm::prove_fact_chain`): whether
    /// it recorded the answer.
    fn prove_symbol_chain(
        &mut self,
        layout: u32,
        key: PropertyKey,
        prototype: Value,
        depth: u8,
        answer: super::symbol_load_proof::SymbolLoadAnswer,
        holder: Value,
    ) -> bool {
        // A chain met for the first time is only noted: the receiver's own shape lacks the key
        // (`record_absent_own`, which no read answers by). Its second walk proves it. A
        // receiver whose layout is met once, as an object made per call and used as a
        // prototype at once (`Object.create(this)`), then marks no prototype and holds no
        // fact under their subjects for a proof no later read would use.
        if depth > 0
            && let Some(key32) = key.key32()
            && self.symbol_load_proofs.word(layout, key32).is_none()
        {
            self.symbol_load_proofs.record_absent_own(layout, key32);
            return false;
        }
        let Some(subjects) = self.heap.chain_subjects(prototype, depth) else { return false };
        // A holder up the chain that is an object record: its id, plus one; any other holder
        // (an array's, a builtin's) is found by its hops (`Vm::proof_holder`).
        let holder = if depth > 0 { holder.direct_object_index().and_then(|r| self.heap.record_id(r)).map_or(0, |id| id + 1) } else { 0 };
        match self.symbol_load_proofs.record_with_holder(layout, key, answer, holder) {
            Some(entry) => {
                self.hold_fact(entry, &subjects);
                true
            }
            None => false,
        }
    }

    /// The validity cell of the `depth` prototypes from `prototype` (`heap::ChainCell`), each
    /// marked a prototype first: it holds while none of them changes its layout (shape,
    /// prototype link, integrity, recorded descriptors) or dies, whatever other prototypes do.
    /// `None` where one has no subject, so no proof is recorded for that chain.
    pub(super) fn chain_cell(&mut self, prototype: Value, depth: u8) -> Option<crate::heap::ChainCell> {
        let subjects = self.heap.chain_subjects(prototype, depth)?;
        Some(self.heap.dependencies().chain_cell(&subjects))
    }

    /// `chain_cell` for a proof made without marking: every one of the `depth` links from
    /// `prototype` must be a watched prototype already; `None` where one is not.
    pub(super) fn watched_chain_cell(&self, prototype: Value, depth: u8) -> Option<crate::heap::ChainCell> {
        let mut subjects = smallvec::SmallVec::<[u32; crate::heap::INHERITED_FIELD_CACHE_MAX_DEPTH]>::new();
        let mut link = prototype;
        for _ in 0..depth {
            let object = self.heap.object(link)?;
            if !object.was_used_as_prototype() {
                return None;
            }
            subjects.push(object.watched_subject()?);
            link = object.proto();
        }
        Some(self.heap.dependencies().chain_cell(&subjects))
    }

    /// `chain_cell` of the whole chain from `prototype` to its end; `None` past `u8::MAX`
    /// links or where a link is no object.
    pub(super) fn chain_cell_to_end(&mut self, prototype: Value) -> Option<crate::heap::ChainCell> {
        let depth = self.chain_depth(prototype)?;
        self.chain_cell(prototype, depth)
    }

    /// The links of the chain from `prototype` to its end; `None` past `u8::MAX` or where a
    /// link is no object.
    fn chain_depth(&self, prototype: Value) -> Option<u8> {
        let mut depth = 0u8;
        let mut link = prototype;
        while !link.is_null() {
            link = self.heap.object(link)?.proto();
            depth = depth.checked_add(1)?;
        }
        Some(depth)
    }

    /// Makes the data dependency `dependency` retire when a link of the whole chain from
    /// `start` (itself included) changes its layout or dies, each marked a prototype first
    /// (`Dependencies::hold_layouts`); `false` where a link has no subject or a table is full.
    pub fn hold_chain_layouts(&mut self, start: Value, dependency: &std::rc::Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
        let Some(subjects) = self.chain_depth(start).and_then(|depth| self.heap.chain_subjects(start, depth)) else {
            return false;
        };
        self.heap.dependencies().hold_layouts(&subjects, dependency)
    }

    /// Holds fact-table entry `entry` under `subjects`, whose change clears it.
    pub(super) fn hold_fact(&self, entry: u32, subjects: &[u32]) {
        if let Some(store) = self.symbol_load_proofs.store_handle() {
            self.heap.dependencies().prove_fact(store, entry, subjects);
        }
    }

    /// Rewrites the getter fact `prove_symbol_chain` just recorded with where its holder's
    /// shape keeps the getter (`SlotAccessor`), so a hit reads it without the slot attributes.
    /// The rewrite takes the same entry, which the chain's subjects already hold.
    fn refine_getter_fact(&self, layout: u32, key: PropertyKey, depth: u8, slot: u32, holder: Value) {
        use super::symbol_load_proof::{KIND_GETTER, KIND_GETTER_PAIR, KIND_GETTER_SLOT};
        let (Some(key32), Some(data)) = (key.key32(), self.heap.object(holder)) else {
            return;
        };
        let kind = match self.slot_accessor(data.shape(), slot as usize) {
            super::SlotAccessor::Getter => KIND_GETTER_SLOT,
            super::SlotAccessor::Pair => KIND_GETTER_PAIR,
            super::SlotAccessor::Attributes => KIND_GETTER,
        };
        if kind != KIND_GETTER {
            // An own accessor's holder is each receiver itself, never the one recorded.
            let holder_id = if depth == 0 { 0 } else { holder.direct_object_index().and_then(|r| self.heap.record_id(r)).map_or(0, |id| id + 1) };
            self.symbol_load_proofs.record_accessor(layout, key32, depth, slot, kind, holder_id);
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
    pub fn plain_property_lookup(
        &self,
        object: Value,
        key: PropertyKey,
    ) -> Option<Option<Value>> {
        // An array is ordinary for any key but an index and `length`: a symbol, or a name such
        // as `constructor`, which `ArraySpeciesCreate` reads of every array a method copies.
        let array_ordinary = match key {
            PropertyKey::Symbol(_) => true,
            PropertyKey::String(atom) => {
                atom != self.length_atom && !self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
            }
            PropertyKey::Private(..) => false,
        };
        let mut current = object;
        loop {
            let data = self.plain_link(current, key, array_ordinary)?;
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

    /// Where a named read of a boolean or number receiver looks (its primitive prototype),
    /// or the builtin method it answers with directly; any other receiver is its own holder.
    fn primitive_read_holder(&mut self, object: Value, atom: Atom) -> ControlFlow<Value, Value> {
        let builtin = if object.as_bool().is_some() {
            match self.atom_name(atom) {
                "toString" => Native::BooleanToString,
                "valueOf" => Native::BooleanValueOf,
                _ => return ControlFlow::Continue(self.primitive_prototype(object).unwrap_or(self.object_proto)),
            }
        } else if object.as_number().is_some() {
            let prototype = self.primitive_prototype(object).unwrap_or(self.object_proto);
            match atom {
                _ if atom == self.runtime_atoms.to_string => Native::NumberString,
                _ if atom == self.to_fixed_atom => Native::NumberFixed,
                _ if atom == self.to_precision_atom => Native::NumberPrecision,
                _ => return ControlFlow::Continue(prototype),
            }
        } else {
            return ControlFlow::Continue(object);
        };
        let prototype = self.primitive_prototype(object).unwrap_or(self.object_proto);
        match self.own_property(prototype, atom) {
            Some(_) => ControlFlow::Continue(prototype),
            None => ControlFlow::Break(self.native_value(builtin)),
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
        // Only plain named reads record proofs, so a proved getter needs no key checks.
        if let Some(getter) = self.proved_getter(object, PropertyKey::string(atom)) {
            return self.call_value(p, getter, receiver, &[]);
        }
        let private_name = self.is_private_name(atom);
        if object.is_null() || object.is_undefined() {
            let property = self.atom_name(atom).to_owned();
            return Err(self.nullish_property_error(p, object, Some(&property)));
        }
        if !private_name && let Some(read) = self.restricted_function_read(object, atom) {
            return read;
        }
        let private_target = object;
        let mut object = match self.primitive_read_holder(object, atom) {
            ControlFlow::Break(builtin) => return Ok(builtin),
            ControlFlow::Continue(holder) => holder,
        };
        if private_name {
            self.check_private_brand_for_read(p, private_target, atom)?;
            if let Some(value) = self.private_field_value(p, private_target, atom) {
                return Ok(value);
            }
        }
        loop {
            match self.ordinary_chain_step(p, object, atom, receiver, private_name) {
                ControlFlow::Break(read) => return read,
                ControlFlow::Continue(link) => object = link,
            }
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
            // The object's own shape slot is resolved once, for both its accessor flag and its value.
            let own_slot = self.own_shape_slot(object, atom);
            if let Some(attributes) = self.property_attributes_at(object, PropertyKey::string(atom), own_slot)
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
                .heap.object(object)
                .is_some_and(|object| object.is_module_namespace())
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
            if let Some(v) = self.own_property_at(object, atom, own_slot).or_else(|| self.string_wrapper_unit(object, PropertyKey::string(atom))) {
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
            } else if let Some(symbol) = self.heap.symbol_data(object) {
                let description = symbol.clone();
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
                    Some(Cell::ShadowRealm { object: x, .. }) => object = x.proto(),
                    Some(Cell::RegExp { object: x, .. }) => object = x.proto(),
                    Some(Cell::WeakMap { object: x, .. })
                    | Some(Cell::WeakSet { object: x, .. }) => object = x.proto(),
                    Some(Cell::WeakRef { object: x, .. }) => object = x.proto(),
                    Some(Cell::FinalizationRegistry { object: x, .. }) => object = x.proto(),
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
    pub fn private_field_value(&mut self, p: &ResidualProgram, target: Value, atom: Atom) -> Option<Value> {
        let home = self.private_name_home(p, atom)?;
        self.private_field(target, PrivateBrand { home, name: atom })
    }

    /// The private brand of `atom` that function `function` of the running program, whose
    /// frame would capture from `environment`, names when it has no binding sites: the home a
    /// run of it with a frame found (`private_name_home`'s memo), else the initialized home
    /// binding its static name binding reads from `environment` (`direct_capture`'s read), as
    /// an arrow field's closure of each new instance finds its class's. `None` leaves the
    /// access to the general path, which finds the home.
    pub(super) fn memoized_private_brand(
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

    /// The private method or accessor that `object`, which carries `brand`, reads at lane site
    /// `site`: the element the site recorded under the brand's home (`PrivateSiteHomes::element`), else the one `[[Get]]`'s walk of the
    /// object's chain meets, recorded for the next read. `None` leaves the read to the general
    /// path: a missing getter, a proxy on the chain, or a site without a recorded home.
    pub fn site_private_element(
        &self,
        site: usize,
        brand: PrivateBrand,
        object: Value,
    ) -> Option<super::private_site_home::PrivateElement> {
        use super::private_site_home::PrivateElement;
        if let Some(element) = self.private_site_homes.element(site, brand.home) {
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
            link = self.heap.object(link)?.proto();
            if link.is_null() {
                return None;
            }
        };
        self.private_site_homes.record_element(site, brand.home, element);
        Some(element)
    }

    /// `private_field_value` through `memoized_private_brand`.
    pub fn memoized_private_field_value(
        &self,
        p: &ResidualProgram,
        environment: Value,
        function: u32,
        target: Value,
        atom: Atom,
    ) -> Option<Value> {
        let brand = self.memoized_private_brand(p, environment, function, atom)?;
        self.private_field(target, brand)
    }

    /// The store of private field `atom` of `target` under the running frame's home for the
    /// name: `false` for a target without the field, or whose element is a method or
    /// accessor, which the general path throws for or calls.
    pub(super) fn set_private_field_value(&mut self, p: &ResidualProgram, target: Value, atom: Atom, value: Value) -> bool {
        let Some(home) = self.private_name_home(p, atom) else { return false; };
        self.set_private_field(target, PrivateBrand { home, name: atom }, value)
    }

    /// Whether `object` may gain a private element: always, unless the host applies
    /// non-extensibility to private elements (`Host::nonextensible_applies_to_private`).
    pub fn private_element_addable(&self, object: &Object) -> bool {
        object.is_extensible() || !self.host.nonextensible_applies_to_private()
    }

    pub fn initialize_private_field(&mut self, p: &ResidualProgram, target: Value, home: Value, atom: Atom, value: Value) -> Result<(), JsError> {
        // record: private_element_addable takes the record
        let Some(addable) = self.heap.object(target).and_then(|object| object.record_only().map(|record| self.private_element_addable(record))) else {
            return Err(self.type_error(p, "Cannot add private field to a non-object".into()));
        };
        if !addable {
            return Err(self.type_error(p, "Cannot add private field to a non-extensible object".into()));
        }
        if self.add_private_field(target, PrivateBrand { home, name: atom }, value) {
            return Ok(());
        }
        let name = crate::bytecode::private_identity_label(self.atom_name(atom)).unwrap_or_else(|| self.atom_name(atom)).to_owned();
        Err(self.type_error(p, format!("Cannot initialize #{name} twice on the same object")))
    }

    /// `MarkPrivateName`: brands `object` with private method name `atom` of the class home
    /// `home` (PrivateMethodOrAccessorAdd), which raises for a non-extensible object or one
    /// already branded; the class itself, as its own home, takes its static methods' brand.
    pub fn mark_private_name(&mut self, p: &ResidualProgram, object: Value, home: Value, atom: Atom) -> Result<(), JsError> {
        let brand = PrivateBrand { home, name: atom };
        if object != home {
            // record: private_element_addable takes the record
            let Some((addable, branded)) = self.heap.object(object).and_then(|data| data.record_only().map(|record| (self.private_element_addable(record), self.has_private_name(object, brand)))) else {
                return Err(self.type_error(p, "Cannot add private method to a non-extensible object".into()));
            };
            if !addable {
                return Err(self.type_error(p, "Cannot add private method to a non-extensible object".into()));
            }
            if branded {
                return Err(self.type_error(p, "Cannot add private method to an object that already has it".into()));
            }
        }
        self.add_private_name(object, brand);
        Ok(())
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

    /// Report a failed private read with V8/Node's public error wording. Keep
    /// `check_private_brand` unchanged for writes and brand checks whose error
    /// semantics are distinct from reading a private member.
    pub(super) fn check_private_brand_for_read(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Result<(), JsError> {
        if self.has_private_brand(p, target, atom) {
            return Ok(());
        }
        Err(self.undeclared_private_read_error(p, atom))
    }

    /// V8/Node's TypeError for reading private member `atom` from an object without its brand.
    fn undeclared_private_read_error(&mut self, p: &ResidualProgram, atom: Atom) -> JsError {
        let name = self.atom_name(atom);
        let member = if name.starts_with('#') {
            name.to_owned()
        } else {
            format!("#{name}")
        };
        self.type_error(
            p,
            format!(
                "Cannot read private member {member} from an object whose class did not declare it"
            ),
        )
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
        self.has_private_name(target, PrivateBrand { home, name: atom }).then_some(home)
    }

    /// The home object that declares private name `atom` for the running frame: the nearest
    /// eval-introduced private binding, otherwise the first visible class home declaring the
    /// name. It depends on the frame's function and environment chain, never on the target,
    /// so a found home is memoized while that environment lives.
    pub(super) fn private_name_home(&mut self, p: &ResidualProgram, atom: Atom) -> Option<Value> {
        let frame_index = self.frames.len().checked_sub(1)?;
        let owner = VarOwner::Frame(frame_index);
        let start = self.captured_parent_environment(self.owner_view(owner));
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
        if let Some(binding) = self.name_binding(self.owner_view(owner), atom)
            && binding.kind == crate::bytecode::LexicalBindingKind::PrivateName
        {
            let home = self.load_name_binding(p, owner, binding).ok();
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
        while let Some(crate::heap::EnvironmentRecord {
            parent,
            function,
            slots,
            scope,
        }) = self.heap.environment(environment)
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
        let home = homes
            .into_iter()
            .map(|(_, home)| home)
            .find(|home| self.has_private_name(*home, PrivateBrand { home: *home, name: atom }))?;
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
        if let Some(value) = self.private_field_value(p, proxy, atom) {
            return Ok(value);
        }
        let Some(home) = self.private_brand_home(p, proxy, atom) else {
            return Err(self.undeclared_private_read_error(p, atom));
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

/// What a proving walk (`Vm::prove_load`) found.
pub(in crate::vm) enum ProvedLoad {
    /// The key's data value.
    Value(Value),
    /// No link holds the key.
    Absent,
    /// The key is an accessor, with its getter if it has one.
    Getter(Option<Value>),
    /// A link no proof covers: exotic, dictionary or descriptor-held.
    Unproved,
}
