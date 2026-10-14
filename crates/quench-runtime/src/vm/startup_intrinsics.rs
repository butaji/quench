//! The realm's intrinsics as the host bootstrap left them. A host builds its builtin modules
//! with these originals in place, the way Node's builtins use primordials captured before
//! user code runs, so a script's mutation of an intrinsic never reaches builtin loading and a
//! builtin can be built on first use instead of at startup.

use super::object_descriptors::PropertyDescriptorRecord;
use super::*;

/// One own property of an intrinsic holder and its descriptor when the snapshot was sealed.
#[derive(Clone, Copy)]
struct IntrinsicProperty {
    holder: Value,
    key: Value,
    original: PropertyDescriptorRecord,
}

#[derive(Default)]
pub(super) struct StartupIntrinsics {
    properties: Vec<IntrinsicProperty>,
    /// Open `enter_startup_intrinsics` calls; only the outermost one swaps.
    depth: usize,
    /// Properties holding their original during the outermost call, with the descriptor the
    /// script had given them (`None` when it had deleted the property).
    swapped: Vec<(usize, Option<PropertyDescriptorRecord>)>,
}

impl StartupIntrinsics {
    /// Values the snapshot keeps alive: holders, keys and both descriptors of a swap.
    pub(super) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        let record_values = |record: &PropertyDescriptorRecord| {
            [record.value, record.getter, record.setter]
                .into_iter()
                .flatten()
        };
        self.properties
            .iter()
            .flat_map(move |property| {
                [property.holder, property.key]
                    .into_iter()
                    .chain(record_values(&property.original))
            })
            .chain(
                self.swapped
                    .iter()
                    .filter_map(|(_, previous)| previous.as_ref())
                    .flat_map(record_values),
            )
    }
}

fn same_descriptor(left: &PropertyDescriptorRecord, right: &PropertyDescriptorRecord) -> bool {
    left.value == right.value
        && left.writable == right.writable
        && left.enumerable == right.enumerable
        && left.configurable == right.configurable
        && left.getter == right.getter
        && left.setter == right.setter
}

impl<H: Host> Vm<H> {
    /// Records the intrinsics: every own property of the global object, of each object a
    /// global property holds, and of that object's `prototype`.
    pub(crate) fn seal_startup_intrinsics(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let globals = self.realm.globals;
        let mut holders = vec![globals];
        let mut seen = FxHashSet::from_iter([globals]);
        let prototype_atom = self.intern_atom("prototype");
        for key in self.object_own_key_values(p, globals)? {
            let Some(record) = self.own_property_record(p, globals, key)? else {
                continue;
            };
            let Some(value) = record.value.filter(|value| self.is_object_like(*value)) else {
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
            let keys = self.object_own_key_values(p, holder)?;
            self.with_call_roots(keys.iter().copied(), |vm| {
                for &key in &keys {
                    if let Some(original) = vm.own_property_record(p, holder, key)? {
                        properties.push(IntrinsicProperty {
                            holder,
                            key,
                            original,
                        });
                    }
                }
                Ok::<_, JsError>(())
            })?;
        }
        properties.shrink_to_fit();
        self.startup_intrinsics.properties = properties;
        Ok(())
    }

    /// Puts every recorded intrinsic back to its sealed descriptor until the matching
    /// `leave_startup_intrinsics`. A property the script made non-configurable keeps its value.
    pub(crate) fn enter_startup_intrinsics(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        self.startup_intrinsics.depth += 1;
        if self.startup_intrinsics.depth > 1 {
            return Ok(());
        }
        for index in 0..self.startup_intrinsics.properties.len() {
            let property = self.startup_intrinsics.properties[index];
            let current = self.own_property_record(p, property.holder, property.key)?;
            if current.is_some_and(|current| same_descriptor(&current, &property.original)) {
                continue;
            }
            // Recorded first: once overwritten, the script's values are rooted only here.
            self.startup_intrinsics.swapped.push((index, current));
            if !self.define_own_property_record(p, property.holder, property.key, property.original)? {
                self.startup_intrinsics.swapped.pop();
            }
        }
        Ok(())
    }

    /// Returns the script's own descriptors after the outermost `enter_startup_intrinsics`. A
    /// recorded property the host changed meanwhile keeps the change, as its new original.
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
            let result = match previous {
                Some(previous) => self
                    .define_own_property_record(p, property.holder, property.key, previous)
                    .map(|_| ()),
                None => self
                    .object_delete_property(p, &[property.holder, property.key])
                    .map(|_| ()),
            };
            outcome = outcome.and(result);
        }
        self.startup_intrinsics.swapped.clear();
        for index in 0..self.startup_intrinsics.properties.len() {
            if restored[index] {
                continue;
            }
            let property = self.startup_intrinsics.properties[index];
            if let Some(current) = self.own_property_record(p, property.holder, property.key)?
                && !same_descriptor(&current, &property.original)
            {
                self.startup_intrinsics.properties[index].original = current;
            }
        }
        outcome
    }

    /// `holder`'s own descriptor for `key`, as `Object.getOwnPropertyDescriptor` reports it.
    fn own_property_record(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: Value,
    ) -> Result<Option<PropertyDescriptorRecord>, JsError> {
        let descriptor = self.object_get_own_property_descriptor(p, &[holder, key])?;
        if descriptor.is_undefined() {
            return Ok(None);
        }
        Ok(Some(self.own_descriptor_record(descriptor)))
    }
}
