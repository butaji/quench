//! How an object's shape changes: a property added, redefined or deleted moves it along the shape
//! transition tree (`transition_shape`), and deleting the property it just added returns it to the
//! shape it had.
use super::*;

impl Vm {
    #[quench_macros::test_visible]
    pub(in crate::vm) fn transition_shape(&mut self, shape: u32, atom: Atom) -> u32 {
        self.transition_property_shape(shape, PropertyKey::string(atom))
    }
    pub(in crate::vm) fn transition_property_shape(&mut self, shape: u32, key: PropertyKey) -> u32 {
        self.transition_data_property_shape(shape, key, DEFAULT_DATA_ATTRIBUTES)
    }
    /// Out of line and of plain values: a lane construct that inlined it would otherwise pass
    /// the transition it builds by its stack address, a temporary the construct's common path
    /// then does not hold.
    #[inline(never)]
    pub fn transition_data_property_shape(
        &mut self,
        shape: u32,
        key: PropertyKey,
        attributes: DataAttributes,
    ) -> u32 {
        debug_assert!(
            self.walked_property_shape_slot(shape, key).is_none(),
            "a shape gains only keys it lacks"
        );
        let transition = ShapeTransition::Add {
            key,
            slot: u32::try_from(self.shapes[shape as usize].storage_len())
                .expect("object property index exceeds u32"),
            attributes,
        };
        self.append_cached_shape_transition(shape, transition)
    }
    pub(in crate::vm) fn append_cached_shape_transition(
        &mut self,
        shape: u32,
        transition: ShapeTransition,
    ) -> u32 {
        if let Some(next) = self.owned_dictionary_edit(shape, transition) {
            return next;
        }
        let key = transition.cache_key();
        if let Some(key) = key {
            let last = self.shapes[shape as usize].last_child.get();
            let next = if last != NO_LAST_CHILD
                && crate::vm::shape_transitions::is_transition(&self.shapes, last, shape, key)
            {
                Some(last)
            } else {
                self.transitions.get(&self.shapes, shape, key)
            };
            if let Some(next) = next {
                self.shapes[shape as usize].last_child.set(next);
                self.profile.shape_transition(true);
                self.carry_lookup_index(shape, next);
                return next;
            }
        }
        if key.is_some() || matches!(transition, ShapeTransition::Add { .. }) {
            self.profile.shape_transition(false);
        }
        let next_storage_len = transition_storage_len(transition, self.shapes[shape as usize].storage_len());
        self.append_transition_shape(shape, key, transition, next_storage_len)
    }
    /// The shape an object of `shape` had before `shape` added `key` at `slot`, its last slot,
    /// when deleting `key` can return the object to it: a plain add from an ordinary shape,
    /// then only redefinitions of that slot (an accessor is added as data, then given its
    /// descriptor).
    fn shape_before_last_add(&self, shape: u32, key: PropertyKey, slot: usize) -> Option<u32> {
        let mut added_shape = shape;
        while let ShapeTransition::Descriptor { slot: redefined, .. } = self.shapes[added_shape as usize].transition {
            let parent = self.shapes[added_shape as usize].parent;
            if redefined as usize != slot || self.shapes[parent as usize].storage_len() != self.shapes[added_shape as usize].storage_len() {
                return None;
            }
            added_shape = parent;
        }
        let ShapeTransition::Add { key: added, slot: added_slot, .. } = self.shapes[added_shape as usize].transition else {
            return None;
        };
        let parent = self.shapes[added_shape as usize].parent;
        (added == key && added_slot as usize == slot && self.shape_appends_one_slot(parent, added_shape))
            .then_some(parent)
    }

    /// Deletes `key`, whose slot of the object's shape is already `Value::DELETED` (`delete_shape_slot`). The object's
    /// shape moves on: back to the shape before the key's add when it was the last-added key, else along a `Delete`
    /// transition. A class cell takes either change in place: its class moves, or it turns dictionary
    /// (`DictionaryTrigger::DeletionPattern`) and the key goes from its table. No delete departs an object.
    pub(in crate::vm) fn delete_shape_property(&mut self, object: Value, key: PropertyKey) {
        let Some(shape) = self.heap.object(object).map(|object| object.shape()) else { return };
        let Some(slot) = self.property_shape_slot(shape, key) else {
            return;
        };
        if let Some(parent) = self.shape_before_last_add(shape, key, slot) {
            self.return_to_parent_shape(object, parent);
            self.reconfigure_function_name(object, key);
            self.invalidate_method_caches_for_key(key);
            return;
        }
        if self.heap.live_class_cell(object).is_some() {
            // The cell's fields move into its table, which then takes the delete as a record's does.
            self.mark_object_dictionary(object, DictionaryTrigger::DeletionPattern);
        }
        let shape = self.heap.object(object).map_or(shape, |object| object.shape());
        let next_id =
            self.append_cached_shape_transition(shape, ShapeTransition::Delete { key, slot: slot as u32 });
        self.heap.set_object_shape(object, next_id);
        self.pack_owned_dictionary(object, next_id);
        self.reconfigure_function_name(object, key);
        self.invalidate_method_caches_for_key(key);
    }

    /// The object returns to `parent`, the shape it had before its last-added key, so transient keys added and deleted in
    /// turn (an object used as a stack of entries) fork no shapes. A class cell takes that class in place
    /// (`Heap::class_cell_drop_last`). A record cuts its arena run to the parent's length.
    fn return_to_parent_shape(&mut self, object: Value, parent: u32) {
        if let Some(granule) = self.heap.live_class_cell(object) {
            return self.heap.class_cell_drop_last(granule, parent);
        }
        // record-only: the arena run of the record's values
        let data = self.heap.object(object).expect("object survived property deletion").record_only().expect("object survived property deletion");
        let kept = self.shapes[parent as usize].storage_len();
        if !crate::value_vec::ValueArena::drops_last_in_place(data.properties(), kept + 1) {
            // The shorter length names a smaller arena range, so the values move to one;
            // the object owns the same edges, which its write barrier already logged.
            let values = (0..kept)
                .map(|index| self.heap.property_get(data, index).unwrap_or(Value::DELETED))
                .collect::<Vec<_>>();
            self.heap.initialize_object_properties(object, parent, &values);
        }
        self.heap.set_object_shape(object, parent);
    }
}

/// The storage length of the shape `transition` makes from one of `storage_len`: an add takes a slot.
fn transition_storage_len(transition: ShapeTransition, storage_len: usize) -> usize {
    match transition {
        ShapeTransition::Add { .. } => storage_len
            .checked_add(1)
            .filter(|length| u32::try_from(*length).is_ok())
            .expect("object property storage exhausted"),
        ShapeTransition::Descriptor { .. } | ShapeTransition::Delete { .. } | ShapeTransition::FunctionName { .. } | ShapeTransition::FunctionNameReconfigured | ShapeTransition::FunctionLengthReconfigured | ShapeTransition::Constructed { .. } => storage_len,
        ShapeTransition::Root
        | ShapeTransition::Vacant
        | ShapeTransition::Dictionary { .. } => {
            unreachable!("cached shape transitions are adds, deletions, descriptors or metadata")
        }
    }
}
