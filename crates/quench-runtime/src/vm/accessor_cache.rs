//! The accessor a site's recorded entry answers (`Vm::record_accessor_field_cache`).
use super::object::OWN_ACCESSOR;
use super::*;

impl<H: Host> Vm<H> {
    /// The getter a field read of `atom` at cache index `site` calls on `object`, when the
    /// site's recorded accessor still answers it.
    pub(super) fn cached_field_getter(&self, object: Value, atom: Atom, site: usize) -> Option<Value> {
        self.cached_field_accessor(object, atom, site)?.0
    }

    /// Whether the accessor the site's recorded entry answers for `object` lives on its
    /// immediate prototype, whose values a hint's watch covers (`hold_getter_hint`).
    pub(super) fn cached_accessor_on_prototype(&self, object: Value, atom: Atom, site: usize) -> bool {
        self.accessor_receiver(object, atom)
            .and_then(|receiver| Some((self.accessor_entry(receiver, site)?, receiver.proto())))
            .is_some_and(|(cache, prototype)| cache.holder != OWN_ACCESSOR && cache.holder == prototype)
    }

    /// The accessor functions the site's recorded entry answers for `object` (a store's
    /// setter, a read's getter): its getter, and its setter where it has one.
    pub(super) fn cached_field_accessor(&self, object: Value, atom: Atom, site: usize) -> Option<ShapeAccessors> {
        // A site that never met an accessor has no entry to match, whatever the receiver.
        if self.accessor_field_indices.get(site) == NO_ACCESSOR_FIELD {
            return None;
        }
        let receiver = self.accessor_receiver(object, atom)?;
        let cache = self.accessor_entry(receiver, site)?;
        let holder = if cache.holder == OWN_ACCESSOR {
            (!receiver.has_side_descriptors()).then_some(receiver)?
        } else {
            self.heap.ordinary_object(cache.holder)?
        };
        let held = self.heap.property_get(holder, usize::from(cache.slot));
        match cache.slot_accessor {
            SlotAccessor::Getter => held.map(|getter| (Some(getter), None)),
            SlotAccessor::Pair => match self.heap.get(held?) {
                Some(Cell::AccessorPair { getter, setter }) => Some((Some(*getter), Some(*setter))),
                _ => None,
            },
            SlotAccessor::Attributes => self
                .slot_attributes(holder, usize::from(cache.slot))
                .map(|attributes| (attributes.getter, attributes.setter)),
        }
    }

    /// The object whose shape an accessor entry may match. An ordinary object's is its own:
    /// an entry is recorded only for a shape `shape_property_lookup` answers (never a private
    /// name's, a dictionary's or a module namespace's, whose shape is its own), so a shape
    /// that matches one has been proved, and the lookup's checks are not made again. Any other
    /// receiver is checked as it was recorded.
    #[inline(always)]
    fn accessor_receiver(&self, object: Value, atom: Atom) -> Option<&Object> {
        match self.heap.object_record(object) {
            Some(record) => Some(record),
            None => self.shape_property_lookup(object, atom),
        }
    }

    /// The site's entry for a receiver of `receiver`'s shape and prototype, while the
    /// prototype layout epoch it was proved under still holds.
    #[inline(always)]
    fn accessor_entry(&self, receiver: &Object, site: usize) -> Option<&AccessorFieldCache> {
        let mut index = self.accessor_field_indices.get(site);
        let epoch = crate::heap::prototype_layout_epoch();
        while index != NO_ACCESSOR_FIELD {
            let cache = self.accessor_fields.get(index as usize)?;
            if receiver.shape() == cache.receiver && receiver.proto() == cache.prototype && epoch == cache.epoch {
                return Some(cache);
            }
            index = cache.next;
        }
        None
    }
}
