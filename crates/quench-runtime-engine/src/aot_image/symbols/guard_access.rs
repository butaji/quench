//! The access a shape guard admits (`GuardAccess`) and the guard key that packs it with its recipe and slot.

/// The bits of a guard key below its recipe (`guard_key`): the access, then the slot.
pub(super) const GUARD_SLOT_BITS: u32 = 12;
/// The bits of a guard key below its access (`GuardAccess`): the slot.
pub(super) const GUARD_ACCESS_SHIFT: u32 = GUARD_SLOT_BITS - GuardAccess::BITS;

/// What the access a shape guard admits needs of its slot (`guarded_shape`): a read reads any
/// tagged word; a store writes in place, with no join of the slot's type, only the values of one
/// class (`StoreArm::of` of the slot typed `GuardAccess::stored`), so its guard resolves only where
/// the slot's type holds every value of that class. A prototype read guards two shapes: its
/// receiver's (`Inherited`), which owns no field of the name, and its holder's (`Holder`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum GuardAccess {
    Read,
    /// A store of an int32.
    StoreInt32,
    /// A store of a direct object or `null`.
    StoreReference,
    /// A store of any tagged word: the slot is admitted to every value (`ahead_images::fields`).
    StoreTagged,
    /// The receiver of a read its prototype answers: the recipe's shape, whatever its slots hold.
    Inherited,
    /// The holder of a prototype read: the class cell the recipe's `prototype` names, where the
    /// slot holds the recipe's one field, the name read, as a plain data field (`holder_cell`). The
    /// recipe names no shape of its own: it carries the holder and the name.
    Holder,
    /// A read of a slot every store of which writes an int32 (`AotFacts::field_type`): the slot is
    /// admitted to int32s, as its stores' guards admit it, and the read takes its words as int32s
    /// under the slot's dependency (`ahead_images::guard_dependencies`), so a store of anything else
    /// retires the code: the type is proven where the slot is written, never at the read.
    ReadInt32,
    /// A read of a slot every store of which writes a direct object or `null`, as `ReadInt32`.
    ReadReference,
}

impl GuardAccess {
    /// The bits of the access in a guard key.
    pub(super) const BITS: u32 = 3;
    pub(super) const ALL: [Self; 8] = [Self::Read, Self::StoreInt32, Self::StoreReference, Self::StoreTagged, Self::Inherited, Self::Holder,
        Self::ReadInt32, Self::ReadReference];

    /// The type the slot's type must hold for the access: `None` for a read.
    pub(crate) fn stored(self) -> Option<quench_runtime::typed_ir::Type> {
        use quench_runtime::typed_ir::Type;
        match self {
            Self::Read | Self::Inherited | Self::Holder | Self::ReadInt32 | Self::ReadReference => None,
            Self::StoreInt32 => Some(Type::INT32),
            Self::StoreReference => Some(Type::DIRECT_OBJECT_FAMILY.join(Type::language(quench_runtime::ValueType::Null))),
            Self::StoreTagged => Some(Type::ANY),
        }
    }

    /// The type a typed read takes its slot's words as, which the slot's type must be within: `None`
    /// for any other access.
    pub(crate) fn read(self) -> Option<quench_runtime::typed_ir::Type> {
        use quench_runtime::typed_ir::Type;
        match self {
            Self::ReadInt32 => Some(Type::INT32),
            Self::ReadReference => Some(Type::NULLABLE_DIRECT_OBJECT),
            _ => None,
        }
    }

    /// The accesses of a field whose stores all write values of type `ty` (`AotFacts::field_type`):
    /// its reads' and its stores', typed by the one class `ty` is within, or any tagged word.
    pub(crate) fn of_field(ty: quench_runtime::typed_ir::Type) -> (Self, Self) {
        use quench_runtime::typed_ir::Type;
        if ty == Type::NEVER { (Self::Read, Self::StoreTagged) }
        else if ty.is_subtype(Type::INT32) { (Self::ReadInt32, Self::StoreInt32) }
        else if ty.is_subtype(Type::NULLABLE_DIRECT_OBJECT) { (Self::ReadReference, Self::StoreReference) }
        else { (Self::Read, Self::StoreTagged) }
    }
}

/// The type the guard `key` admits its slot's words as for a typed read (`GuardAccess::read`).
pub(crate) fn guard_read_type(key: u32) -> Option<quench_runtime::typed_ir::Type> {
    GuardAccess::ALL.get(((key >> GUARD_ACCESS_SHIFT) & ((1 << GuardAccess::BITS) - 1)) as usize)?.read()
}

/// The shape recipe guard key `key` guards a slot of (`guard_key`).
pub(crate) fn guard_recipe(key: u32) -> u32 {
    key >> GUARD_SLOT_BITS
}

/// The key of the guard of `access` of `slot` of shape recipe `recipe`: the recipe above the
/// access and the slot. `None` where either does not fit.
pub(super) fn guard_key(recipe: u32, slot: u16, access: GuardAccess) -> Option<u32> {
    (u32::from(slot) < 1 << GUARD_ACCESS_SHIFT && recipe < 1 << (u32::BITS - GUARD_SLOT_BITS))
        .then_some(recipe << GUARD_SLOT_BITS | (access as u32) << GUARD_ACCESS_SHIFT | u32::from(slot))
}
