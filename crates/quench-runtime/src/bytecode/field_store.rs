use super::{Op, Register, Traits, WideInstruction};

/// Where a named field store finds its receiver: field B's register, or the frame's `this`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreReceiver {
    Register,
    This,
}

/// A named field store's receiver and whether a rejected write throws. The opcode table owns
/// this, so the interpreter and every tier decode a store from one definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldStore {
    pub receiver: StoreReceiver,
    pub strict: bool,
}

impl Op {
    #[inline(always)]
    pub const fn field_store(self) -> Option<FieldStore> {
        if !self.is_field_store() {
            return None;
        }
        let receiver = if self.traits().has(Traits::FIELD_STORE_THIS) {
            StoreReceiver::This
        } else {
            StoreReceiver::Register
        };
        Some(FieldStore { receiver, strict: self.traits().has(Traits::FIELD_STORE_STRICT) })
    }

    /// Whether this is a strict named field store (`SetFieldStrict`, `SetThisFieldStrict`).
    #[inline(always)]
    pub(crate) const fn is_strict_field_store(self) -> bool {
        matches!(self.field_store(), Some(FieldStore { strict: true, .. }))
    }

    #[inline(always)]
    pub const fn is_field_store(self) -> bool {
        self.traits().has(Traits::FIELD_STORE)
    }

    /// Whether this reads or stores a named field (`GetField` or a field store).
    pub(crate) const fn is_named_field_access(self) -> bool {
        self.traits().has(Traits::FIELD_LEAF) || self.is_field_store()
    }
}

impl WideInstruction {
    /// A named field store's receiver register, or `None` for a store to `this` or an
    /// instruction that is no field store.
    #[inline(always)]
    pub fn field_store_register(self) -> Option<Register> {
        match self.op().field_store()?.receiver {
            StoreReceiver::Register => Some(self.register_b()),
            StoreReceiver::This => None,
        }
    }

    /// Whether this is a named field store to the frame's `this`.
    pub fn stores_to_this(self) -> bool {
        self.op().field_store().is_some_and(|store| store.receiver == StoreReceiver::This)
    }
}
