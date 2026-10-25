//! The realm's intrinsics as the host bootstrap left them. A host builds its builtin modules
//! with these originals in place, the way Node's builtins use primordials captured before
//! user code runs, so a script's mutation of an intrinsic never reaches builtin loading and a
//! builtin can be built on first use instead of at startup.
//!
//! Properties are read from their shape slots, so recording and comparing allocate nothing; a
//! key becomes a JavaScript value only when a property actually has to be redefined. A holder is
//! recorded as its shape and slot values: the shape already names each property's key, slot and
//! attributes, so the snapshot costs one value per property, and a holder still in that shape
//! with those values compares without lookups.

use super::object_descriptors::PropertyDescriptorRecord;
use super::*;

/// A shape-held property's state: the slot value (unused by an accessor) and its attributes.
#[derive(Clone, Copy, PartialEq)]
struct SlotProperty {
    value: Value,
    attributes: PropertyAttributes,
}

impl SlotProperty {
    fn values(&self) -> impl Iterator<Item = Value> {
        [
            Some(self.value),
            self.attributes.getter,
            self.attributes.setter,
        ]
        .into_iter()
        .flatten()
    }
}

/// An intrinsic holder as the snapshot was sealed: its shape, which gives every own property's
/// key, slot and attributes, and its slot values. Shape compaction keeps the shape alive.
struct SealedHolder {
    holder: Value,
    shape: u32,
    values: Box<[Value]>,
}

/// A property holding its original during the outermost `enter_startup_intrinsics`, with the
/// state the script had given it (`None` when it had deleted the property).
#[derive(Clone, Copy)]
struct SwappedProperty {
    holder: Value,
    key: PropertyKey,
    previous: Option<SlotProperty>,
}

#[derive(Default)]
pub(super) struct StartupIntrinsics {
    holders: Vec<SealedHolder>,
    /// The sealed shapes' symbol keys and accessor functions: fixed once sealed, so derived
    /// then rather than walked at every collection. The holders and their values are roots
    /// read from `holders` itself, not copied here.
    shape_roots: Box<[Value]>,
    /// Open `enter_startup_intrinsics` calls; only the outermost one swaps.
    depth: usize,
    swapped: Vec<SwappedProperty>,
    regexp: Option<super::regexp::LegacyRegExpSnapshot>,
}

impl StartupIntrinsics {
    /// The sealed shapes, which compaction must keep.
    pub(super) fn sealed_shapes(&self) -> impl Iterator<Item = u32> + '_ {
        self.holders.iter().map(|holder| holder.shape)
    }

    /// Follows shape compaction (`mapping` from old to new shape ids).
    pub(super) fn remap_shapes(&mut self, mapping: &[u32]) {
        for holder in &mut self.holders {
            holder.shape = mapping[holder.shape as usize];
        }
    }
}

impl<H: Host> Vm<H> {
    /// Read a captured data property without temporarily changing guest state.
    /// This also works after a script makes its replacement non-configurable.
    pub(crate) fn startup_data_property(&self, holder: Value, name: &str) -> Option<Value> {
        let key = PropertyKey::string(self.lookup_atom(name)?);
        let index = self
            .startup_intrinsics
            .holders
            .iter()
            .position(|sealed| sealed.holder == holder)?;
        let (_, original) = self
            .sealed_properties(index)
            .into_iter()
            .find(|(sealed_key, _)| *sealed_key == key)?;
        (!original.attributes.accessor).then_some(original.value)
    }

    pub(crate) fn has_startup_global_name(&self, name: &str) -> bool {
        let Some(atom) = self.lookup_atom(name) else {
            return false;
        };
        self.startup_intrinsics
            .holders
            .iter()
            .filter(|holder| holder.holder == self.realm.globals)
            .any(|holder| {
                super::gc::active_shape_attributes(&self.shapes, holder.shape)
                    .into_iter()
                    .flatten()
                    .any(|(key, _)| key == PropertyKey::string(atom))
            })
    }

    /// Values the snapshot keeps alive: holders, their sealed values, the symbol keys and
    /// accessor functions of their sealed shapes, and the script's state of a swap.
    pub(super) fn startup_intrinsic_roots(&self) -> impl Iterator<Item = Value> + '_ {
        let intrinsics = &self.startup_intrinsics;
        intrinsics
            .holders
            .iter()
            .flat_map(|sealed| std::iter::once(sealed.holder).chain(sealed.values.iter().copied()))
            .chain(intrinsics.shape_roots.iter().copied())
            .chain(
                intrinsics
                    .swapped
                    .iter()
                    .flat_map(|swapped| swapped.previous.iter().flat_map(SlotProperty::values)),
            )
    }

    fn shape_roots(&self, holders: &[SealedHolder]) -> Box<[Value]> {
        let mut roots = Vec::new();
        for sealed in holders {
            for (key, attributes) in super::gc::active_shape_attributes(&self.shapes, sealed.shape)
                .into_iter()
                .flatten()
            {
                // `values` already holds a slot getter.
                let (getter, setter) = attributes.functions(&self.shape_accessors, || None);
                roots.extend(key.symbol_value().into_iter().chain(getter).chain(setter));
            }
        }
        roots.into_boxed_slice()
    }

    /// Records the intrinsics: every own shape property of the global object, of each object a
    /// global property holds, and of that object's `prototype`.
    pub(crate) fn seal_startup_intrinsics(&mut self) {
        let globals = self.realm.globals;
        let prototype_atom = self.intern_atom("prototype");
        let mut holders = vec![globals];
        let mut seen = FxHashSet::from_iter([globals]);
        for key in self.own_shape_keys(globals) {
            let Some(value) = self
                .slot_property(globals, key)
                .filter(|property| !property.attributes.accessor)
                .map(|property| property.value)
                .filter(|value| self.is_object_like(*value))
            else {
                continue;
            };
            let prototype = self
                .own_property(value, prototype_atom)
                .filter(|prototype| self.is_object_like(*prototype));
            for holder in std::iter::once(value).chain(prototype) {
                if seen.insert(holder) {
                    holders.push(holder);
                }
            }
        }
        let sealed: Vec<_> = holders
            .into_iter()
            .filter_map(|holder| self.sealed_holder(holder))
            .collect();
        self.startup_intrinsics.shape_roots = self.shape_roots(&sealed);
        self.startup_intrinsics.holders = sealed;
    }

    /// `holder`'s current shape and slot values.
    fn sealed_holder(&self, holder: Value) -> Option<SealedHolder> {
        let object = self.object_data(holder)?;
        let shape = object.shape();
        let values = (0..self.shapes[shape as usize].storage_len())
            .map(|slot| {
                self.heap
                    .property_get(object, slot)
                    .unwrap_or(Value::UNDEFINED)
            })
            .collect();
        Some(SealedHolder {
            holder,
            shape,
            values,
        })
    }

    /// Whether sealed holder `index` is still in its sealed shape with its sealed values.
    fn holder_unchanged(&self, index: usize) -> bool {
        let sealed = &self.startup_intrinsics.holders[index];
        self.object_data(sealed.holder).is_some_and(|object| {
            object.shape() == sealed.shape
                && sealed.values.iter().enumerate().all(|(slot, value)| {
                    self.heap
                        .property_get(object, slot)
                        .unwrap_or(Value::UNDEFINED)
                        == *value
                })
        })
    }

    /// Sealed holder `index`'s recorded properties: key and original state.
    fn sealed_properties(&self, index: usize) -> Vec<(PropertyKey, SlotProperty)> {
        let sealed = &self.startup_intrinsics.holders[index];
        super::gc::active_shape_attributes(&self.shapes, sealed.shape)
            .into_iter()
            .enumerate()
            .filter_map(|(slot, entry)| {
                let (key, attributes) = entry?;
                let raw = sealed.values.get(slot).copied();
                let value = if attributes.slot_getter() { None } else { raw };
                let attributes = attributes.attributes(&self.shape_accessors, || raw);
                let value = value.unwrap_or(Value::UNDEFINED);
                Some((key, SlotProperty { value, attributes }))
            })
            .collect()
    }

    /// Puts every recorded intrinsic back to its sealed state until the matching
    /// `leave_startup_intrinsics`. A property the script made non-configurable keeps its state.
    pub(crate) fn enter_startup_intrinsics(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        self.startup_intrinsics.depth += 1;
        if self.startup_intrinsics.depth > 1 {
            return Ok(());
        }
        self.startup_intrinsics.regexp = Some(self.snapshot_regexp_legacy());
        for index in 0..self.startup_intrinsics.holders.len() {
            if self.holder_unchanged(index) {
                continue;
            }
            let holder = self.startup_intrinsics.holders[index].holder;
            for (key, original) in self.sealed_properties(index) {
                let current = self.slot_property(holder, key);
                if current == Some(original) {
                    continue;
                }
                // Recorded first: once overwritten, the script's values are rooted only here.
                self.startup_intrinsics.swapped.push(SwappedProperty {
                    holder,
                    key,
                    previous: current,
                });
                if !self.redefine_slot_property(p, holder, key, original)? {
                    self.startup_intrinsics.swapped.pop();
                }
            }
        }
        Ok(())
    }

    /// Returns the script's own state after the outermost `enter_startup_intrinsics`. A holder
    /// the host changed meanwhile (a lazy data property it materialized) keeps the change: its
    /// state, with the originals still in place, becomes its sealed state.
    pub(crate) fn leave_startup_intrinsics(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        self.startup_intrinsics.depth -= 1;
        if self.startup_intrinsics.depth > 0 {
            return Ok(());
        }
        let mut resealed = false;
        for index in 0..self.startup_intrinsics.holders.len() {
            if !self.holder_unchanged(index) {
                let holder = self.startup_intrinsics.holders[index].holder;
                if let Some(sealed) = self.sealed_holder(holder) {
                    self.startup_intrinsics.holders[index] = sealed;
                    resealed = true;
                }
            }
        }
        if resealed {
            self.startup_intrinsics.shape_roots =
                self.shape_roots(&self.startup_intrinsics.holders);
        }
        // The swap list stays traced until every script value is back in place.
        let swapped = self.startup_intrinsics.swapped.clone();
        let mut outcome = Ok(());
        for swap in swapped.iter().rev() {
            let result = match swap.previous {
                Some(previous) => self
                    .redefine_slot_property(p, swap.holder, swap.key, previous)
                    .map(|_| ()),
                None => {
                    let key = self.property_key_value(swap.key);
                    self.object_delete_property(p, &[swap.holder, key])
                        .map(|_| ())
                }
            };
            outcome = outcome.and(result);
        }
        self.startup_intrinsics.swapped.clear();
        if let Some(snapshot) = self.startup_intrinsics.regexp.take() {
            self.restore_regexp_legacy(snapshot);
        }
        outcome
    }

    fn own_shape_keys(&self, holder: Value) -> Vec<PropertyKey> {
        self.object_data(holder)
            .map(|object| self.shape_keys(object.shape()))
            .unwrap_or_default()
    }

    /// `holder`'s own shape-held property `key`, read without allocating.
    fn slot_property(&self, holder: Value, key: PropertyKey) -> Option<SlotProperty> {
        let (_, slot) = self.object_property_slot(holder, key)?;
        let object = self.object_data(holder)?;
        let attributes = self.slot_attributes(object, slot)?;
        let value = self
            .slot_data_value(object, slot)
            .unwrap_or(Value::UNDEFINED);
        Some(SlotProperty { value, attributes })
    }

    /// Restore a trusted snapshot, including a guest's non-configurable replacement.
    /// This internal scope must capture the startup value even after a guest freezes
    /// its replacement. Ordinary guest definitions still use normal descriptor rules;
    /// both entering and leaving this scope restore complete descriptor state.
    fn redefine_slot_property(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: PropertyKey,
        property: SlotProperty,
    ) -> Result<bool, JsError> {
        if let Some(mut attributes) = self.property_attributes(holder, key) {
            if !attributes.configurable {
                attributes.configurable = true;
                self.set_property_attributes(holder, key, attributes);
            }
        }
        let extensible = self.object_data(holder).is_some_and(Object::is_extensible);
        if !extensible {
            self.object_data_mut(holder)
                .expect("sealed intrinsic holder")
                .set_extensible(true);
        }
        let key = self.property_key_value(key);
        let record = PropertyDescriptorRecord::from_attributes(property.value, property.attributes);
        let result = self.with_call_roots([key], |vm| {
            vm.define_own_property_record(p, holder, key, record)
        });
        if !extensible {
            self.object_data_mut(holder)
                .expect("sealed intrinsic holder")
                .set_extensible(false);
        }
        result
    }

    /// `key` as a JavaScript property key value.
    fn property_key_value(&mut self, key: PropertyKey) -> Value {
        match key {
            PropertyKey::String(atom) | PropertyKey::Private(atom) => {
                self.heap.alloc_string(self.atom_value(atom))
            }
            PropertyKey::Symbol(symbol) => symbol,
        }
    }
}

impl StartupIntrinsics {
    /// The sealed holders for a snapshot (`crate::snapshot`); a snapshot is taken outside every
    /// `enter_startup_intrinsics`, so nothing is swapped.
    pub(super) fn write_snapshot(
        &self,
        writer: &mut crate::snapshot::Writer,
    ) -> crate::snapshot::SnapshotResult<()> {
        let Self {
            holders,
            shape_roots,
            depth,
            swapped,
            regexp,
        } = self;
        if *depth != 0 || !swapped.is_empty() || regexp.is_some() {
            return Err(crate::snapshot::SnapshotError::unsupported(
                "a startup intrinsics scope",
            ));
        }
        writer.len(holders.len());
        for SealedHolder {
            holder,
            shape,
            values,
        } in holders
        {
            writer.value(*holder);
            writer.u32(*shape);
            writer.values(values);
        }
        writer.values(shape_roots);
        Ok(())
    }

    pub(super) fn read_snapshot(
        reader: &mut crate::snapshot::Reader<'_>,
    ) -> crate::snapshot::SnapshotResult<Self> {
        let holders = crate::snapshot::read_list(reader, |reader| {
            Ok(SealedHolder {
                holder: reader.value()?,
                shape: reader.u32()?,
                values: reader.values()?.into(),
            })
        })?;
        Ok(Self {
            holders,
            shape_roots: reader.values()?.into(),
            depth: 0,
            swapped: Vec::new(),
            regexp: None,
        })
    }
}
