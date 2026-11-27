use super::*;

/// Keys an object lists, held inline up to a count most objects stay within, so listing
/// them allocates no buffer beside the array that takes them.
pub(super) type EnumerableKeys<T> = smallvec::SmallVec<[T; INLINE_ENUMERABLE_KEYS]>;
/// Object literals and class instances mostly hold up to eight properties.
const INLINE_ENUMERABLE_KEYS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnumerableOwnPropertyKind {
    Key,
    Value,
    KeyValue,
}

impl EnumerableOwnPropertyKind {
    /// What `Object.keys`, `Object.values` or `Object.entries` lists.
    pub fn of(native: Native) -> Option<Self> {
        match native {
            Native::ObjectKeys => Some(Self::Key),
            Native::ObjectValues => Some(Self::Value),
            Native::ObjectEntries => Some(Self::KeyValue),
            _ => None,
        }
    }
}

impl Vm {
    /// EnumerableOwnProperties of an object whose shape answers it: `Object.keys` takes the key
    /// memo first (`plain_key_list`), `Object.values` and `Object.entries` the shape's data items
    /// (`shape_enumerable_data_items`). No guest code runs and nothing raises. `None` leaves
    /// Proxies, accessors and every other object to the general path.
    pub fn plain_enumerable_own_properties(&mut self, object: Value, kind: EnumerableOwnPropertyKind) -> Option<Value> {
        if kind == EnumerableOwnPropertyKind::Key {
            return self.plain_key_list(object);
        }
        self.shape_enumerable_data_items(object, kind)
    }

    /// `Object.keys` of an object whose shape answers it: the memoized key strings of its shape
    /// (`plain_object_keys`) on a hit, else the shape's enumerable keys made into strings.
    fn plain_key_list(&mut self, object: Value) -> Option<Value> {
        if let Some(keys) = self.plain_object_keys(object) {
            return Some(keys);
        }
        // The shape's key strings, made once per shape, copied into the result in one part; the
        // allocation does not collect, so the memo's strings stay live through the copy.
        if let Some(keys) = self.own_enumerable_key_values_cached(object) {
            return Some(self.array_of(&keys));
        }
        let keys: EnumerableKeys<Value> =
            self.shape_enumerable_keys(object)?.into_iter().map(|atom| self.atom_string(atom)).collect();
        Some(self.array_of(&keys))
    }

    pub(super) fn indexed_name_keys(&mut self, object: Value) -> Option<Vec<Value>> {
        let (indices, array_length) = if self.heap.array(object).is_some() {
            (
                self.array_present_indices(object),
                self.own_array_length(object).is_some(),
            )
        } else if matches!(self.heap.get(object), Some(Cell::TypedArray { .. })) {
            ((0..self.typed_array_length(object)?).collect(), false)
        } else {
            ((0..self.string_wrapper_length(object)?).collect(), false)
        };
        let mut keys = indices
            .into_iter()
            .map(|index| self.heap.alloc_string(index.to_string().into()))
            .collect::<Vec<_>>();
        if array_length {
            keys.push(self.heap.alloc_string("length".into()));
        }
        Some(keys)
    }

    /// EnumerableOwnProperties(O, key) for an object whose string keys all live in its shape
    /// (not a Proxy, array, typed array or module namespace): enumerability is read from the
    /// shape in OrdinaryOwnPropertyKeys order, without key strings or descriptor objects.
    /// `None` sends every other object through the generic path.
    pub(super) fn shape_enumerable_keys(&self, object: Value) -> Option<EnumerableKeys<Atom>> {
        if let Some(slots) = self.shape_enumerable_slots(object) {
            return Some(slots.iter().map(|(atom, _)| *atom).collect());
        }
        self.shape_enumerable_key_walk(object)
            .map(|slots| EnumerableKeys::from_vec(slots.into_iter().map(|(atom, _)| atom).collect()))
    }

    /// The own enumerable string keys of `object` as atoms, in OrdinaryOwnPropertyKeys order,
    /// memoized per shape (`enumerable_key_lists`). Shapes never change, so an entry holds for
    /// the shape's life; compaction and `invalidate_field_caches` clear the list. `None` for
    /// every object `shape_enumerable_slots` declines (Proxy, array, typed array, String
    /// wrapper, module namespace, side descriptors, dictionary shapes), which keep the general path.
    pub(crate) fn own_enumerable_keys_cached(&mut self, object: Value) -> Option<std::rc::Rc<[Atom]>> {
        // A memo entry was made from an object its shape answers for; a Proxy, array or other
        // exotic object can carry the same shape (a Proxy's record keeps the empty root shape),
        // so the test comes before the lookup, not only before the fill.
        if !self.shape_lists_enumerable_keys(object) {
            return None;
        }
        // Side descriptors are record data.
        let data = self.heap.object(object)?;
        let shape = data.shape();
        if data.record_only().is_some_and(|record| record.has_side_descriptors()) || self.shape_is_dictionary(shape) {
            return None;
        }
        if let Some(Some(keys)) = self.enumerable_key_lists.get(shape as usize) {
            return Some(keys.clone());
        }
        let keys: std::rc::Rc<[Atom]> = self.shape_enumerable_slots(object)?.iter().map(|&(atom, _)| atom).collect();
        let index = shape as usize;
        if self.enumerable_key_lists.len() <= index {
            self.enumerable_key_lists.resize(index + 1, None);
        }
        self.enumerable_key_lists[index] = Some(keys.clone());
        Some(keys)
    }

    /// The same keys as `own_enumerable_keys_cached`, as string values, memoized per shape
    /// (`enumerable_key_value_lists`). The heap does not keep an atom's string, so the memo
    /// holds no string across a collection: `collect_inner` clears it with the constructed-shape
    /// memo, as do compaction, `invalidate_field_caches` and program reset. A value is valid
    /// until the next collection, and a caller that holds one across a collection roots it as
    /// it would any other value. `None` wherever `own_enumerable_keys_cached` is `None`.
    pub(crate) fn own_enumerable_key_values_cached(&mut self, object: Value) -> Option<std::rc::Rc<[Value]>> {
        if !self.shape_lists_enumerable_keys(object) {
            return None;
        }
        // Side descriptors are record data.
        let data = self.heap.object(object)?;
        let shape = data.shape();
        if data.record_only().is_some_and(|record| record.has_side_descriptors()) || self.shape_is_dictionary(shape) {
            return None;
        }
        self.own_enumerable_key_values_of_shape(object, shape)
    }

    /// `own_enumerable_key_values_cached` for `object` whose `shape` the caller has tested (a
    /// non-dictionary shape with no side descriptors), so the record is not read again.
    pub(in crate::vm) fn own_enumerable_key_values_of_shape(&mut self, object: Value, shape: u32) -> Option<std::rc::Rc<[Value]>> {
        if let Some(Some(values)) = self.enumerable_key_value_lists.get(shape as usize) {
            return Some(values.clone());
        }
        let keys = self.own_enumerable_keys_cached(object)?;
        let values: std::rc::Rc<[Value]> = keys.iter().map(|&atom| self.atom_string(atom)).collect();
        let index = shape as usize;
        if self.enumerable_key_value_lists.len() <= index {
            self.enumerable_key_value_lists.resize(index + 1, None);
        }
        self.enumerable_key_value_lists[index] = Some(values.clone());
        Some(values)
    }

    /// `shape_enumerable_keys` with each key's slot, for an object whose shape alone decides
    /// them (`enumerable_key_memo`): a non-dictionary shape without side-table descriptors.
    pub(super) fn shape_enumerable_slots(
        &self,
        object: Value,
    ) -> Option<super::enumerable_key_memo::EnumerableSlots> {
        if !self.shape_lists_enumerable_keys(object) {
            return None;
        }
        // Side descriptors are record data.
        let data = self.heap.object(object)?;
        if data.record_only().is_some_and(|record| record.has_side_descriptors()) || self.shape_is_dictionary(data.shape()) {
            return None;
        }
        self.shape_enumerable_slots_checked(object, data)
    }

    /// `shape_enumerable_slots` for `object`, whose record `data` the caller has read and held to
    /// its eligibility checks already (`shape_lists_enumerable_keys`, no side descriptors, no
    /// dictionary shape): the memo, or a walk of its named slots on a miss. No check and no
    /// record lookup repeats, so a caller that checked the object once (`adopt_source_shape`)
    /// reads its slots through this.
    pub(super) fn shape_enumerable_slots_checked(
        &self,
        object: Value,
        data: &Object,
    ) -> Option<super::enumerable_key_memo::EnumerableSlots> {
        let shape = data.shape();
        if let Some(slots) = self.enumerable_keys.get(shape) {
            return Some(slots);
        }
        let entries = self
            .own_named_slots(object)
            .into_iter()
            .filter(|&(atom, slot)| self.own_slot_enumerable(object, atom, slot))
            .map(|(atom, slot)| Some((atom, u32::try_from(slot).ok()?)))
            .collect::<Option<Vec<(Atom, u32)>>>()?;
        let storage_len = self.shapes[shape as usize].storage_len();
        let slots = super::enumerable_key_memo::EnumerableSlots::new(entries.into(), storage_len);
        self.enumerable_keys.record(shape, slots.clone());
        Some(slots)
    }

    /// `Object.keys(object)` with each key's value, where reading them runs no guest code: an
    /// object whose enumerable own keys its shape lists (`shape_enumerable_slots`), each a data
    /// property, each key as its text. `None` leaves the reads to guest order.
    pub(crate) fn plain_enumerable_entries(&self, object: Value) -> Option<Vec<(String, Value)>> {
        let slots = self.shape_enumerable_slots(object)?;
        let data = self.heap.object(object)?;
        slots
            .iter()
            .map(|&(atom, slot)| {
                let slot = slot as usize;
                // Slot attributes are record data.
                if self.slot_attributes(data, slot)?.accessor {
                    return None;
                }
                Some((self.atom_value(atom).host_string().to_owned(), self.heap.property_get_ref(data, slot)?))
            })
            .collect()
    }

    /// Whether `object`'s string keys all live in its shape: no Proxy, array, typed array,
    /// String wrapper or module namespace.
    pub(super) fn shape_lists_enumerable_keys(&self, object: Value) -> bool {
        !matches!(self.heap.get(object), Some(Cell::Proxy { .. } | Cell::TypedArray { .. }))
            && self.heap.array(object).is_none()
            && self.string_wrapper_primitive(object).is_none()
            && self.heap.object(object).is_some_and(|data| !data.is_module_namespace())
    }

    /// The enumerable own string keys of `object` with their slots, in OrdinaryOwnPropertyKeys
    /// order, each slot read once (`own_named_slots`) and its enumerability from that slot's
    /// attributes (`own_slot_enumerable`), which no second resolve of the key repeats.
    fn shape_enumerable_key_walk(&self, object: Value) -> Option<Vec<(Atom, usize)>> {
        if !self.shape_lists_enumerable_keys(object) {
            return None;
        }
        Some(
            self.own_named_slots(object)
                .into_iter()
                .filter(|&(atom, slot)| self.own_slot_enumerable(object, atom, slot))
                .collect(),
        )
    }

    /// `Object.values` or `Object.entries` of an object whose enumerable keys its shape lists
    /// (`shape_enumerable_slots`), all data properties: each value is a slot read, observing
    /// nothing. Each item is kept in the call roots while the next allocates. `None` leaves
    /// keys, accessors and every other object to the general path.
    fn shape_enumerable_data_items(&mut self, object: Value, kind: EnumerableOwnPropertyKind) -> Option<Value> {
        let slots = self.shape_enumerable_slots(object)?;
        let data = self.heap.object(object)?;
        // A shape whose chain added no accessor (`shape_may_have_accessors`) holds data
        // properties only, so no slot needs its attributes read.
        let data_only = !self.shape_may_have_accessors(data.shape());
        let values = slots
            .iter()
            .map(|&(_, slot)| {
                let slot = slot as usize;
                (data_only || self.slot_attributes(data, slot).is_some_and(|attributes| !attributes.accessor))
                    .then(|| self.heap.property_get_ref(data, slot))
                    .flatten()
            })
            .collect::<Option<smallvec::SmallVec<[Value; 8]>>>()?;
        let start = self.active_call_roots.len();
        self.active_call_roots.push(object);
        let mut items = smallvec::SmallVec::<[Value; 8]>::with_capacity(values.len());
        for (&(atom, _), value) in slots.iter().zip(values) {
            let item = match kind {
                EnumerableOwnPropertyKind::KeyValue => {
                    let key = self.atom_string(atom);
                    self.active_call_roots.push(key);
                    self.array_of(&[key, value])
                }
                _ => value,
            };
            self.active_call_roots.push(item);
            items.push(item);
        }
        let result = self.array_of(&items);
        self.active_call_roots.truncate(start);
        Some(result)
    }

    /// The general EnumerableOwnProperties of `object` (boxed first): each own string key's
    /// enumerability and value through [[Get]]. The plain body (`plain_enumerable_own_properties`)
    /// is tried by the caller before this, so this is the fallback for what it declines.
    pub(super) fn enumerable_own_properties(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        kind: EnumerableOwnPropertyKind,
    ) -> Result<Value, JsError> {
        let object = self.box_object(object)?;
        let object = self.heap.root(object);
        let mut keys = Vec::new();
        let mut properties = Vec::new();
        let outcome = (|| {
            keys = self
                .object_own_key_values(p, self.heap.root_value(object).unwrap())?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            for key in &keys {
                let property = self.heap.root_value(*key).unwrap();
                if !self.heap.is_string(property) {
                    continue;
                }
                let enumerable = self.own_property_enumerable(
                    p,
                    self.heap.root_value(object).unwrap(),
                    property,
                )?;
                if enumerable != Some(true) {
                    continue;
                }
                let item = match kind {
                    EnumerableOwnPropertyKind::Key => self.heap.root_value(*key).unwrap(),
                    EnumerableOwnPropertyKind::Value | EnumerableOwnPropertyKind::KeyValue => self
                        .get_index(
                            p,
                            self.heap.root_value(object).unwrap(),
                            self.heap.root_value(*key).unwrap(),
                        )?,
                };
                let property = match kind {
                    EnumerableOwnPropertyKind::KeyValue => self.heap.alloc_array(crate::heap::ArrayObject::new(
                            Self::empty_object(self.array_proto),
                            vec![self.heap.root_value(*key).unwrap(), item],
                        )),
                    EnumerableOwnPropertyKind::Key | EnumerableOwnPropertyKind::Value => item,
                };
                properties.push(self.heap.root(property));
            }
            let elements = properties
                .iter()
                .map(|root| self.heap.root_value(*root).unwrap())
                .collect::<Vec<_>>();
            Ok(self.heap.alloc_array(crate::heap::ArrayObject::new(
                    Self::empty_object(self.array_proto),
                    elements,
                )))
        })();
        for root in keys.into_iter().chain(properties) {
            self.heap.release_root(root);
        }
        self.heap.release_root(object);
        outcome
    }


    pub(super) fn object_names(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        let values = self
            .object_own_key_values(p, object)?
            .into_iter()
            .filter(|key| self.heap.is_string(*key))
            .collect::<Vec<_>>();
        Ok(self.heap.alloc_array(crate::heap::ArrayObject::new(
                Self::empty_object(self.array_proto),
                values,
            )))
    }
}
