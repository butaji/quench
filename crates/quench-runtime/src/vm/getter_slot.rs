//! Getters that return a field of their receiver, as `get x() { return this.x; }` and
//! `get x() { return this.#x; }` compile: a lone `GetField` of `this` that returns.
//!
//! An accessor entry (`AccessorFieldCache`) proves the receiver's shape and the chain to the
//! holder of the getter. When that getter is such a body, the entry also records the
//! receiver's own data slot the body reads (`AccessorFields::returns`). A read the entry answers then loads that slot under the entry's
//! guards and the getter's identity, with no call. Every receiver of the shape holds the field
//! at that slot. A private name's slot is recorded only while one home declared it, so its key
//! is the name's own atom (`PrivateKeys::single`).

use super::{Atom, Host, Value, Vm};
use crate::heap::{FunctionKind, FunctionRecord, Object};
use super::{AccessorFieldCache, NO_ACCESSOR_FIELD, object::OWN_ACCESSOR};

/// An accessor entry whose getter returns no receiver slot (`AccessorFields::returns`).
pub(super) const NO_RETURNED_SLOT: u16 = u16::MAX;

/// The accessor entries (`Vm::accessor_fields`, which this derefs to) and, per entry, the
/// getter it names and the receiver slot that getter returns.
#[derive(Default)]
pub(super) struct AccessorFields {
    entries: Vec<AccessorFieldCache>,
    returns: Vec<(Value, u16)>,
}

impl std::ops::Deref for AccessorFields {
    type Target = Vec<AccessorFieldCache>;
    fn deref(&self) -> &Self::Target { &self.entries }
}

impl std::ops::DerefMut for AccessorFields {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.entries }
}

impl<H: Host> Vm<H> {
    /// The getter `cache` names on `holder`, as its slot keeps it (`SlotAccessor`).
    pub(super) fn accessor_entry_getter(&self, cache: &AccessorFieldCache, holder: &Object) -> Option<Value> {
        match cache.slot_accessor {
            super::SlotAccessor::Getter => self.heap.property_get(holder, usize::from(cache.slot)),
            super::SlotAccessor::Pair => match self.heap.get(self.heap.property_get(holder, usize::from(cache.slot))?) {
                Some(crate::heap::Cell::AccessorPair { getter, .. }) => Some(*getter),
                _ => None,
            },
            super::SlotAccessor::Attributes => self.slot_attributes(holder, usize::from(cache.slot))?.getter,
        }
    }

    /// Records, for the accessor entry at `index`, the getter it names and the receiver slot
    /// that getter returns, or `NO_RETURNED_SLOT`.
    pub(super) fn note_accessor_return(&mut self, index: u32, entry: &AccessorFieldCache) {
        let returned = self.returned_slot(entry).unwrap_or((Value::UNDEFINED, NO_RETURNED_SLOT));
        let index = index as usize;
        if self.accessor_fields.returns.len() <= index {
            self.accessor_fields.returns.resize(index + 1, (Value::UNDEFINED, NO_RETURNED_SLOT));
        }
        self.accessor_fields.returns[index] = returned;
    }

    /// The getter of a prototype-held accessor entry and the own data slot of the entry's
    /// receiver shape it returns.
    fn returned_slot(&self, entry: &AccessorFieldCache) -> Option<(Value, u16)> {
        if entry.holder == OWN_ACCESSOR || self.shape_is_dictionary(entry.receiver) {
            return None;
        }
        let getter = self.accessor_entry_getter(entry, self.heap.ordinary_object(entry.holder)?)?;
        let atom = self.returned_field_atom(getter)?;
        if self.is_private_name(atom) && !self.private_homes.fields.single(atom) {
            return None;
        }
        let slot = self.shape_slot(entry.receiver, atom)?;
        let attributes = self.shape_slot_attributes(entry.receiver, slot)?;
        let data = !attributes.slot_accessors() && !attributes.table_accessors();
        Some((getter, u16::try_from(slot).ok().filter(|_| data)?))
    }

    /// The name `getter`'s body returns of `this`: a lone returning `GetField` of `this`.
    fn returned_field_atom(&self, getter: Value) -> Option<Atom> {
        let Some(FunctionRecord { kind: FunctionKind::User(program, id), .. }) = self.heap.function(getter) else {
            return None;
        };
        let program = self.programs.get(*program)?;
        let body = program.function_get(*id as usize)?;
        if body.is_arrow || body.is_class_constructor || !body.handlers.is_empty() {
            return None;
        }
        let first = super::dispatch_fast::decoded_instruction(body, *body.code.first()?);
        let returns_this_field = first.op() == crate::bytecode::Op::GetField
            && first.returns_from_frame()
            && first.b() == crate::bytecode::FieldBase::THIS.0;
        returns_this_field.then(|| first.imm())
    }

    /// A read at cache index `site` of `object` that an accessor entry answers with the
    /// receiver slot its getter returns (`note_accessor_return`); `None` for any other read.
    #[inline(always)]
    pub(super) fn returned_slot_read(&self, object: Value, site: usize) -> Option<Value> {
        let mut index = self.accessor_field_indices.get(site);
        let receiver = self.heap.object_record_by_id(object.direct_object_index()?);
        let (shape, proto, epoch) = (receiver.shape(), receiver.proto(), crate::heap::prototype_layout_epoch());
        while index != NO_ACCESSOR_FIELD {
            let cache = self.accessor_fields.get(index as usize)?;
            if shape == cache.receiver && proto == cache.prototype && epoch == cache.epoch {
                let &(getter, slot) = self.accessor_fields.returns.get(index as usize)?;
                let holder = self.heap.ordinary_object(cache.holder).filter(|_| slot != NO_RETURNED_SLOT)?;
                // SAFETY: the entry proved the receiver's shape, whose storage holds `slot`
                // outside dictionary storage (`returned_slot`).
                return (self.accessor_entry_getter(cache, holder)? == getter)
                    .then(|| unsafe { self.heap.property_get_shaped(receiver, usize::from(slot)) })?;
            }
            index = cache.next;
        }
        None
    }
}
