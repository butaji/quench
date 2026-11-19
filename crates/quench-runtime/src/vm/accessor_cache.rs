//! The accessor a site's recorded entry answers (`Vm::record_accessor_field_cache`).
use super::object::OWN_ACCESSOR;
use super::*;

impl<H: Host> Vm<H> {
    /// The getter a field read of `atom` at cache index `site` calls on `object`, when the
    /// site's recorded accessor still answers it.
    pub(super) fn cached_field_getter(&self, object: Value, atom: Atom, site: usize) -> Option<Value> {
        if self.accessor_field_indices.get(site) == NO_ACCESSOR_FIELD {
            return None;
        }
        // The fact table first: one probe for the receiver's layout, whatever entries the
        // site rotates through.
        if let Some(receiver) = self.heap.object_record(object)
            && let Some(getter) = self.fact_accessor(receiver, atom, false)
        {
            return getter;
        }
        self.cached_field_accessor(object, atom, site)?.0
    }

    /// Whether the accessor the site's recorded entry answers for `object` lives on its
    /// immediate prototype, whose values a hint's watch covers (`hold_getter_hint`).
    pub(super) fn cached_accessor_on_prototype(&self, object: Value, atom: Atom, site: usize) -> bool {
        self.receiver_entry(object, atom, site)
            .is_some_and(|(receiver, cache)| cache.holder != OWN_ACCESSOR && cache.holder == receiver.proto())
    }

    /// The accessor functions the site's recorded entry answers for `object` (a store's
    /// setter, a read's getter): its getter, and its setter where it has one.
    pub(super) fn cached_field_accessor(&self, object: Value, atom: Atom, site: usize) -> Option<ShapeAccessors> {
        // A site that never met an accessor has no entry to match, whatever the receiver.
        if self.accessor_field_indices.get(site) == NO_ACCESSOR_FIELD {
            return None;
        }
        let (receiver, cache) = self.receiver_entry(object, atom, site)?;
        self.entry_accessors(cache, receiver)
    }

    /// The accessor functions `cache` names, which proves `receiver`'s shape and chain.
    #[inline(always)]
    fn entry_accessors(&self, cache: &AccessorFieldCache, receiver: &Object) -> Option<ShapeAccessors> {
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

    /// `object`'s record and the site's entry for it, while the cell of the chain it was proved
    /// over still holds. An ordinary receiver matches its entry by layout, which holds its
    /// shape and prototype: a recorded layout is of a shaped receiver outside dictionary mode,
    /// so a match proves what `shape_property_lookup` would.
    fn receiver_entry(&self, object: Value, atom: Atom, site: usize) -> Option<(&Object, &AccessorFieldCache)> {
        if let Some(receiver) = self.heap.object_record(object) {
            let layout = receiver.layout_id();
            return Some((receiver, self.site_entry(site, |cache| layout == cache.layout && cache.cell.holds())?));
        }
        let receiver = self.shape_property_lookup(object, atom)?;
        let (shape, proto) = (receiver.shape(), receiver.proto());
        Some((receiver, self.site_entry(site, |cache| shape == cache.receiver && proto == cache.prototype && cache.cell.holds())?))
    }

    /// The newest of the site's entries that `matches`.
    fn site_entry(&self, site: usize, matches: impl Fn(&AccessorFieldCache) -> bool) -> Option<&AccessorFieldCache> {
        let mut index = self.accessor_field_indices.get(site);
        while index != NO_ACCESSOR_FIELD {
            let cache = self.accessor_fields.get(index as usize)?;
            if matches(cache) {
                return Some(cache);
            }
            index = cache.next;
        }
        None
    }

    /// A getter read at cache index `site` that the site's newest accessor entry memoizes
    /// (`memoize_getter_answer`): the getter's own field read of the receiver, made where the
    /// entry still proves `object`'s shape, prototype and chain and its holder still holds
    /// that getter. `None` leaves the read to `cached_field_getter`.
    #[inline(always)]
    pub(super) fn memoized_getter_answer(&self, object: Value, site: usize) -> Option<Value> {
        let entry = self.accessor_fields.get(self.accessor_field_indices.get(site) as usize)?;
        let (getter, answer) = (entry.getter, entry.answer);
        if getter.is_undefined() {
            return None;
        }
        let receiver = self.heap.object_record_by_id(object.direct_object_index()?);
        if receiver.shape() != entry.receiver
            || receiver.proto() != entry.prototype
            || !entry.cell.holds()
            || self.entry_accessors(entry, receiver)?.0? != getter
        {
            return None;
        }
        self.record_field_entry_hit(answer, object)
    }

    /// Records at the site's newest accessor entry, where it proves `object`, that its
    /// getter `getter` returns the field read `answer` of the receiver
    /// (`memoized_getter_answer`).
    pub(super) fn memoize_getter_answer(&mut self, object: Value, site: usize, getter: Value, answer: FieldCache) {
        let Some(receiver) = object.direct_object_index().map(|id| self.heap.object_record_by_id(id)) else {
            return;
        };
        let (shape, proto) = (receiver.shape(), receiver.proto());
        if self.shape_is_dictionary(shape) {
            return;
        }
        let index = self.accessor_field_indices.get(site);
        if let Some(entry) = self.accessor_fields.get_mut(index as usize)
            && entry.receiver == shape
            && entry.prototype == proto
        {
            (entry.getter, entry.answer) = (getter, answer);
        }
    }
}
