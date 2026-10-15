//! The realm's intrinsics as the host bootstrap left them. A host builds its builtin modules
//! with these originals in place, the way Node's builtins use primordials captured before
//! user code runs, so a script's mutation of an intrinsic never reaches builtin loading and a
//! builtin can be built on first use instead of at startup.
//!
//! Properties are read from their shape slots, so recording and comparing allocate nothing; a
//! key becomes a JavaScript value only when a property actually has to be redefined.

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

/// One own property of an intrinsic holder and its state when the snapshot was sealed.
#[derive(Clone, Copy)]
struct IntrinsicProperty {
    holder: Value,
    key: PropertyKey,
    original: SlotProperty,
}

#[derive(Default)]
pub(super) struct StartupIntrinsics {
    properties: Vec<IntrinsicProperty>,
    /// Open `enter_startup_intrinsics` calls; only the outermost one swaps.
    depth: usize,
    /// Properties holding their original during the outermost call, with the state the script
    /// had given them (`None` when it had deleted the property).
    swapped: Vec<(usize, Option<SlotProperty>)>,
}

impl StartupIntrinsics {
    /// Values the snapshot keeps alive: holders, symbol keys and both states of a swap.
    pub(super) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        self.properties
            .iter()
            .flat_map(|property| {
                [Some(property.holder), property.key.symbol_value()]
                    .into_iter()
                    .flatten()
                    .chain(property.original.values())
            })
            .chain(
                self.swapped
                    .iter()
                    .filter_map(|(_, previous)| previous.as_ref())
                    .flat_map(SlotProperty::values),
            )
    }
}

impl<H: Host> Vm<H> {
    pub(crate) fn has_startup_global_name(&self, name: &str) -> bool {
        self.lookup_atom(name).is_some_and(|atom| {
            self.startup_intrinsics.properties.iter().any(|property|
                property.holder == self.realm.globals && property.key == PropertyKey::string(atom))
        })
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
        let mut properties = Vec::new();
        for holder in holders {
            for key in self.own_shape_keys(holder) {
                if let Some(original) = self.slot_property(holder, key) {
                    properties.push(IntrinsicProperty {
                        holder,
                        key,
                        original,
                    });
                }
            }
        }
        properties.shrink_to_fit();
        self.startup_intrinsics.properties = properties;
    }

    /// Puts every recorded intrinsic back to its sealed state until the matching
    /// `leave_startup_intrinsics`. A property the script made non-configurable keeps its state.
    pub(crate) fn enter_startup_intrinsics(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        self.startup_intrinsics.depth += 1;
        if self.startup_intrinsics.depth > 1 {
            return Ok(());
        }
        for index in 0..self.startup_intrinsics.properties.len() {
            let property = self.startup_intrinsics.properties[index];
            let current = self.slot_property(property.holder, property.key);
            if current == Some(property.original) {
                continue;
            }
            // Recorded first: once overwritten, the script's values are rooted only here.
            self.startup_intrinsics.swapped.push((index, current));
            if !self.redefine_slot_property(p, property.holder, property.key, property.original)? {
                self.startup_intrinsics.swapped.pop();
            }
        }
        Ok(())
    }

    /// Returns the script's own state after the outermost `enter_startup_intrinsics`. A recorded
    /// property the host changed meanwhile keeps the change, as its new original, whether or not
    /// the script had replaced it (a lazy data property the host materialized meanwhile).
    pub(crate) fn leave_startup_intrinsics(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        self.startup_intrinsics.depth -= 1;
        if self.startup_intrinsics.depth > 0 {
            return Ok(());
        }
        // The swap list stays traced until every script value is back in place.
        let swapped = self.startup_intrinsics.swapped.clone();
        let mut restored = vec![false; self.startup_intrinsics.properties.len()];
        let mut outcome = Ok(());
        for &(index, previous) in swapped.iter().rev() {
            restored[index] = true;
            let property = self.startup_intrinsics.properties[index];
            if let Some(current) = self.slot_property(property.holder, property.key)
                && current != property.original
            {
                self.startup_intrinsics.properties[index].original = current;
            }
            let result = match previous {
                Some(previous) => self
                    .redefine_slot_property(p, property.holder, property.key, previous)
                    .map(|_| ()),
                None => {
                    let key = self.property_key_value(property.key);
                    self.object_delete_property(p, &[property.holder, key])
                        .map(|_| ())
                }
            };
            outcome = outcome.and(result);
        }
        self.startup_intrinsics.swapped.clear();
        for (index, restored) in restored.into_iter().enumerate() {
            let property = self.startup_intrinsics.properties[index];
            if !restored
                && let Some(current) = self.slot_property(property.holder, property.key)
                && current != property.original
            {
                self.startup_intrinsics.properties[index].original = current;
            }
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
        let (shape, slot) = self.object_property_slot(holder, key)?;
        let attributes = self.shape_attribute(shape, slot)?;
        let value = self
            .object_data(holder)
            .and_then(|object| self.heap.property_get(object, slot))
            .unwrap_or(Value::UNDEFINED);
        Some(SlotProperty { value, attributes })
    }

    /// Defines `key` on `holder` with `property`'s state, as `Object.defineProperty` would.
    fn redefine_slot_property(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: PropertyKey,
        property: SlotProperty,
    ) -> Result<bool, JsError> {
        let key = self.property_key_value(key);
        let record = PropertyDescriptorRecord::from_attributes(property.value, property.attributes);
        self.with_call_roots([key], |vm| {
            vm.define_own_property_record(p, holder, key, record)
        })
    }

    /// `key` as a JavaScript property key value.
    fn property_key_value(&mut self, key: PropertyKey) -> Value {
        match key {
            PropertyKey::String(atom) | PropertyKey::Private(atom) => {
                self.heap.alloc(Cell::String(self.atom_value(atom)))
            }
            PropertyKey::Symbol(symbol) => symbol,
        }
    }
}
