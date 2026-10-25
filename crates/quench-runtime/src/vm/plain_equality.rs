//! Structural equality of plain data, decided from heap facts without running guest code.
use super::property_key::PropertyKey;
use super::*;

/// The nesting the plain comparison recurses through; deeper data, and cycles, are left
/// unproved. Test fixtures and protocol payloads nest a few levels.
const PLAIN_EQUAL_DEPTH: usize = 64;

impl<H: Host> Vm<H> {
    /// Whether `left` and `right` are provably the same plain data: SameValue, or plain
    /// containers of the same kind whose members are pairwise the same plain data.
    ///
    /// A plain array is a dense, hole-free array of `%Array.prototype%` with no own property
    /// besides its indices and `length`. A plain object is an ordinary object of
    /// `%Object.prototype%`: not an error, boxed primitive, raw JSON or module namespace,
    /// whose own properties are string-keyed data properties, none named `constructor`.
    /// Two plain objects are compared on their own enumerable keys, in any order. Both
    /// prototypes must read `constructor` as a data property and have no `@@toStringTag`.
    ///
    /// Reading such data observes nothing, so a structural comparison that branches on the
    /// constructor, the tag and the enumerable keys (Node's `isDeepStrictEqual`) reaches
    /// this answer. `false` proves nothing: other values, cycles and nesting beyond
    /// `PLAIN_EQUAL_DEPTH` are left to the caller's general comparison.
    pub(crate) fn plain_data_equal(&self, left: Value, right: Value) -> bool {
        if self.same_value(left, right) {
            return true;
        }
        let constructor = self.lookup_atom("constructor");
        if !self.plain_data_prototypes_intact(constructor) {
            return false;
        }
        self.plain_equal_values(left, right, constructor, &mut Vec::new())
    }

    fn plain_data_prototypes_intact(&self, constructor: Option<Atom>) -> bool {
        let Some(constructor) = constructor else {
            return false;
        };
        let Some(tag) = self.well_known_symbols.get(WellKnownSymbol::ToStringTag) else {
            return false;
        };
        [self.object_proto, self.array_proto]
            .into_iter()
            .all(|prototype| {
                self.own_data_property(prototype, PropertyKey::string(constructor))
                    && self.plain_property_lookup(prototype, PropertyKey::symbol(tag)) == Some(None)
            })
    }

    /// Whether `object` owns `key` as a data property its shape records. `%Array.prototype%`
    /// is an array, whose non-index string keys are ordinary.
    fn own_data_property(&self, object: Value, key: PropertyKey) -> bool {
        let Some(data) = self.heap.object(object) else {
            return false;
        };
        let shape = data.shape();
        self.property_shape_slot(shape, key).is_some_and(|slot| {
            !(self.shape_may_have_accessors(shape)
                && self
                    .slot_attributes(data, slot)
                    .is_none_or(|attributes| attributes.accessor))
                && !(data.has_side_descriptors() && self.descriptors.contains_key(&(object, key)))
        })
    }

    fn plain_equal_values(
        &self,
        left: Value,
        right: Value,
        constructor: Option<Atom>,
        ancestors: &mut Vec<Value>,
    ) -> bool {
        if self.same_value(left, right) {
            return true;
        }
        if ancestors.len() == PLAIN_EQUAL_DEPTH || ancestors.contains(&left) {
            return false;
        }
        ancestors.push(left);
        let equal = if self.heap.array(left).is_some() {
            self.plain_equal_arrays(left, right, constructor, ancestors)
        } else {
            self.plain_equal_objects(left, right, constructor, ancestors)
        };
        ancestors.pop();
        equal
    }

    fn plain_array_elements(&self, value: Value) -> Option<&[Value]> {
        let array = self.heap.array(value)?;
        if array.proto() != self.array_proto
            || array.has_side_descriptors()
            || !self.shape_entries(array.shape()).is_empty()
        {
            return None;
        }
        self.present_dense_elements(value, 0..array.elements().len())
    }

    fn plain_equal_arrays(
        &self,
        left: Value,
        right: Value,
        constructor: Option<Atom>,
        ancestors: &mut Vec<Value>,
    ) -> bool {
        let (Some(left), Some(right)) = (
            self.plain_array_elements(left),
            self.plain_array_elements(right),
        ) else {
            return false;
        };
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(&a, &b)| self.plain_equal_values(a, b, constructor, ancestors))
    }

    fn plain_object(
        &self,
        value: Value,
        constructor: Option<Atom>,
    ) -> Option<&crate::heap::Object> {
        let data = self.heap.object_record(value)?;
        let shape = data.shape();
        let plain = data.proto() == self.object_proto
            && !data.is_raw_json()
            && !data.is_module_namespace()
            && data.deferred_module().is_none()
            && !data.has_side_descriptors()
            && !self.shape_may_have_accessors(shape)
            && !self.error_is_error(value)
            && self.boxed_primitive_data(value).is_none()
            && !self
                .shape_entries(shape)
                .iter()
                .any(|(key, _)| matches!(key, PropertyKey::Symbol(_)))
            && constructor.is_none_or(|atom| {
                self.property_shape_slot(shape, PropertyKey::string(atom))
                    .is_none()
            });
        plain.then_some(data)
    }

    fn plain_equal_objects(
        &self,
        left: Value,
        right: Value,
        constructor: Option<Atom>,
        ancestors: &mut Vec<Value>,
    ) -> bool {
        let (Some(left_data), Some(right_data)) = (
            self.plain_object(left, constructor),
            self.plain_object(right, constructor),
        ) else {
            return false;
        };
        let (Some(left_slots), Some(right_slots)) = (
            self.shape_enumerable_slots(left),
            self.shape_enumerable_slots(right),
        ) else {
            return false;
        };
        if left_slots.len() != right_slots.len() {
            return false;
        }
        let same_shape = left_data.shape() == right_data.shape();
        // Equal counts and every enumerable key of `left` an enumerable key of `right`: the
        // same key sets.
        left_slots.iter().all(|&(atom, slot)| {
            let right_slot = if same_shape {
                Some(slot as usize)
            } else {
                self.property_shape_slot(right_data.shape(), PropertyKey::string(atom)).filter(|&slot| {
                    self.slot_attributes(right_data, slot).is_some_and(|attributes| attributes.enumerable)
                })
            };
            let values = (self.heap.property_get(left_data, slot as usize), right_slot.and_then(|slot| self.heap.property_get(right_data, slot)));
            matches!(values, (Some(a), Some(b)) if self.plain_equal_values(a, b, constructor, ancestors))
        })
    }
}
