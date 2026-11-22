//! The accessor a site's recorded entry answers (`Vm::record_accessor_field_cache`).
use super::object::OWN_ACCESSOR;
use super::property_key::PropertyKey;
use super::*;

impl Vm {
    /// The getter a field read of `atom` at cache index `site` calls on `object`, when the
    /// site's recorded accessor still answers it.
    pub fn cached_field_getter(&self, object: Value, atom: Atom, site: usize) -> Option<Value> {
        if self.accessor_field_indices.get(site) == NO_ACCESSOR_FIELD {
            return None;
        }
        // The fact table first: one probe for the receiver's layout, whatever entries the
        // site rotates through.
        if let Some(receiver) = self.heap.object_record(object)
            && let Some(getter) = self.fact_accessor(receiver, PropertyKey::string(atom), false)
        {
            return getter;
        }
        self.cached_field_accessor(object, atom, site)?.0
    }

    /// The getter a read of `atom` calls on `object` once `getter_entry_read` missed at cache
    /// index `site`. For an object record that walk already took the site's entries, which
    /// name no getter, so only the fact table is probed: `cached_field_getter` would walk the
    /// same entries again. Any other receiver has no entry walk yet, so `cached_field_getter`
    /// makes it.
    #[inline(always)]
    pub fn cached_getter_after_miss(&self, object: Value, atom: Atom, site: usize) -> Option<Value> {
        let Some(receiver) = self.heap.object_record(object) else {
            return self.cached_field_getter(object, atom, site);
        };
        if self.accessor_field_indices.get(site) == NO_ACCESSOR_FIELD {
            return None;
        }
        self.fact_accessor(receiver, PropertyKey::string(atom), false).flatten()
    }

    /// Whether the accessor the site's recorded entry answers for `object` lives on its
    /// immediate prototype, whose values a hint's watch covers (`hold_getter_hint`).
    pub fn cached_accessor_on_prototype(&self, object: Value, atom: Atom, site: usize) -> bool {
        self.receiver_entry(object, atom, site)
            .is_some_and(|(receiver, cache)| cache.holder != OWN_ACCESSOR && cache.holder == receiver.proto())
    }

    /// The accessor functions the site's recorded entry answers for `object` (a store's
    /// setter, a read's getter): its getter, and its setter where it has one.
    pub fn cached_field_accessor(&self, object: Value, atom: Atom, site: usize) -> Option<ShapeAccessors> {
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

    /// One walk of the site's accessor entries at cache index `site` for `object`
    /// (getters-as-calls.md): the entry of its layout, while the cell of the chain it was proved
    /// over holds, names the getter its holder holds now. Where that is the getter the
    /// entry's forward was recognized from (`AccessorForward`), the forward is the call's
    /// value; else the getter is the one to call, found without the fact table's probe or a
    /// second walk (`cached_getter_after_miss`). `Miss` leaves the read to `cached_getter_after_miss`.
    #[inline(always)]
    pub fn getter_entry_read(&self, object: Value, site: usize) -> GetterEntryRead {
        let mut index = self.accessor_field_indices.get(site);
        if index == NO_ACCESSOR_FIELD {
            return GetterEntryRead::Miss;
        }
        let Some(receiver) = self.heap.object_record(object) else {
            return GetterEntryRead::Miss;
        };
        let layout = receiver.layout_id();
        while index != NO_ACCESSOR_FIELD {
            let Some(entry) = self.accessor_fields.get(index as usize) else {
                return GetterEntryRead::Miss;
            };
            if entry.layout == layout && entry.cell.holds() {
                let Some(getter) = self.entry_accessors(entry, receiver).and_then(|(getter, _)| getter) else {
                    return GetterEntryRead::Miss;
                };
                if entry.getter == getter
                    && let Some(value) = self.forwarded_read(entry.answer, object)
                {
                    return GetterEntryRead::Answer(value, getter);
                }
                return GetterEntryRead::Call(getter);
            }
            index = entry.next;
        }
        GetterEntryRead::Miss
    }

    /// The value a getter's forward (`AccessorForward`) gives for `object`, whose entry's
    /// layout matched: `None` where the forward's own field entry does not hold for it, or
    /// for no read forward.
    #[inline(always)]
    fn forwarded_read(&self, forward: AccessorForward, object: Value) -> Option<Value> {
        match forward {
            AccessorForward::Field(answer) => self.record_field_entry_hit(answer, object),
            AccessorForward::Constant(value) => Some(value),
            AccessorForward::Receiver => Some(object),
            AccessorForward::None | AccessorForward::Store { .. } => None,
        }
    }

    /// Records at the site's accessor entry that proves `object` (any of its ways, not only
    /// the newest: a getter on a prototype is read from receivers of many shapes, as Air's
    /// `Arg` kinds are) that its accessor `getter` (a store site's setter) does what `answer`
    /// says for a receiver of that layout (`getter_entry_read`, `setter_entry_forward`).
    pub fn memoize_getter_answer(&mut self, object: Value, site: usize, getter: Value, answer: AccessorForward) {
        let Some(receiver) = self.heap.object_record(object) else {
            return;
        };
        if self.shape_is_dictionary(receiver.shape()) {
            return;
        }
        let layout = receiver.layout_id();
        let mut index = self.accessor_field_indices.get(site);
        while index != NO_ACCESSOR_FIELD {
            let Some(entry) = self.accessor_fields.get_mut(index as usize) else {
                return;
            };
            if entry.layout == layout && entry.cell.holds() {
                (entry.getter, entry.answer) = (getter, answer);
                return;
            }
            index = entry.next;
        }
    }

    /// What the store site at cache index `site` knows of `setter`'s body for `object`
    /// (`memoize_getter_answer` of a setter): its forward store where the entry of `object`'s
    /// layout recorded one from this setter, `Declined` where it recorded that the setter
    /// forwards nothing, `Unknown` where it recorded neither.
    #[inline(always)]
    pub fn setter_entry_forward(&self, object: Value, site: usize, setter: Value) -> SetterForward {
        let mut index = self.accessor_field_indices.get(site);
        let Some(receiver) = self.heap.object_record(object) else {
            return SetterForward::Unknown;
        };
        let layout = receiver.layout_id();
        while index != NO_ACCESSOR_FIELD {
            let Some(entry) = self.accessor_fields.get(index as usize) else {
                break;
            };
            if entry.layout == layout && entry.cell.holds() {
                return match entry.answer {
                    _ if entry.getter != setter => SetterForward::Unknown,
                    AccessorForward::Store { cache, atom } => SetterForward::Store(cache, atom),
                    _ => SetterForward::Declined,
                };
            }
            index = entry.next;
        }
        SetterForward::Unknown
    }
}

/// What one walk of a read site's accessor entries gives (`Vm::getter_entry_read`).
#[derive(Clone, Copy)]
pub enum GetterEntryRead {
    /// The forwarded value, and the getter whose call it stands for.
    Answer(Value, Value),
    /// The getter to call: the entry holds no forward for it, or its forward's field entry
    /// declined the receiver.
    Call(Value),
    /// No entry proves the receiver.
    Miss,
}

/// What a store site's accessor entry knows of a setter (`Vm::setter_entry_forward`).
#[derive(Clone, Copy)]
pub enum SetterForward {
    /// The setter's body stores its argument to this own field entry of the receiver, of this name.
    Store(FieldCache, Atom),
    /// The setter's body is no forward store: the call runs.
    Declined,
    /// Nothing recorded yet for this setter.
    Unknown,
}
