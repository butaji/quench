use super::{Op, Register, Traits, WideInstruction};

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

/// The pattern of every named field store opcode's catalog index, for a `match` arm over an
/// opcode index that must stay a jump table entry; `Op::field_store` says what each one means.
macro_rules! field_store_op {
    () => {
        $crate::bytecode::index::SetField
            | $crate::bytecode::index::SetFieldStrict
            | $crate::bytecode::index::SetThisField
            | $crate::bytecode::index::SetThisFieldStrict
    };
}
pub(crate) use field_store_op;

impl Op {
    #[inline(always)]
    pub(crate) const fn field_store(self) -> Option<FieldStore> {
        if !(self.traits().has(Traits::SLOPPY_STORE) || self.traits().has(Traits::STRICT_STORE)) {
            return None;
        }
        let receiver = if self.traits().has(Traits::THIS_FIELD_STORE) { StoreReceiver::This } else { StoreReceiver::Register };
        Some(FieldStore { receiver, strict: self.traits().has(Traits::STRICT_STORE) })
    }

    #[inline(always)]
    pub(crate) const fn is_field_store(self) -> bool {
        self.field_store().is_some()
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
