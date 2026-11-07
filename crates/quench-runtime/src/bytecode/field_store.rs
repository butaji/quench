use super::{Op, Register, WideInstruction};

/// Where a named field store finds its receiver: field B's register, or the frame's `this`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreReceiver {
    Register,
    This,
}

/// A named field store's receiver and whether a rejected write throws. The opcode table owns
/// this, so the interpreter and every tier decode a store from one definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldStore {
    pub(crate) receiver: StoreReceiver,
    pub(crate) strict: bool,
}

impl Op {
    pub(crate) const fn field_store(self) -> Option<FieldStore> {
        let (receiver, strict) = match self {
            Self::SetField => (StoreReceiver::Register, false),
            Self::SetFieldStrict => (StoreReceiver::Register, true),
            Self::SetThisField => (StoreReceiver::This, false),
            Self::SetThisFieldStrict => (StoreReceiver::This, true),
            _ => return None,
        };
        Some(FieldStore { receiver, strict })
    }

    pub(crate) const fn is_field_store(self) -> bool {
        self.field_store().is_some()
    }

    /// Whether this reads or stores a named field (`GetField` or a field store).
    pub(crate) const fn is_named_field_access(self) -> bool {
        matches!(self, Self::GetField) || self.is_field_store()
    }
}

impl WideInstruction {
    /// A named field store's receiver register, or `None` for a store to `this` or an
    /// instruction that is no field store.
    pub(crate) fn field_store_register(self) -> Option<Register> {
        match self.op().field_store()?.receiver {
            StoreReceiver::Register => Some(self.register_b()),
            StoreReceiver::This => None,
        }
    }

    /// Whether this is a named field store to the frame's `this`.
    pub(crate) fn stores_to_this(self) -> bool {
        self.op().field_store().is_some_and(|store| store.receiver == StoreReceiver::This)
    }
}
