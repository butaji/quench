//! What a tier asks of an operation before it steps it, as data on the operation: the registers
//! it names, whether it reads the activation's locals, and whether it can leave at the frame's
//! miss edge. A tier decides by these, never by the opcode the operation decoded from.

use super::{NumberOperation, Operation, Register};

impl<W, P> Operation<W, P> {
    /// The registers of a row over registers, locals and `this` (at most two), which the tier
    /// must hold in its window to step it; `None` for any other row.
    pub(crate) const fn registers(&self) -> Option<[Option<Register>; 2]> {
        Some(match *self {
            Self::Move { destination, source } | Self::LogicalNot { destination, source } | Self::Typeof { destination, source }
                | Self::Number(NumberOperation::ToNumeric { destination, source } | NumberOperation::IncDec { destination, source, .. }) =>
                [Some(destination), Some(source)],
            Self::LoadLocalPlain { destination, .. } | Self::LoadThis { destination } | Self::Void { destination } =>
                [Some(destination), None],
            Self::StoreLocalPlain { source, result, .. } => [Some(source), result],
            Self::RequireObjectCoercible { register } => [Some(register), None],
            _ => return None,
        })
    }

    /// Whether the row reads or writes the activation's local slots (`Frame::load_local`), which a
    /// numeric region's activation has none of.
    pub(crate) const fn uses_locals(&self) -> bool {
        matches!(self, Self::LoadLocalPlain { .. } | Self::StoreLocalPlain { .. })
    }

    /// Whether a frame's step of the row can leave at its miss edge: a row that only copies or
    /// defines a constant word never does.
    pub(crate) const fn leaves(&self) -> bool {
        !matches!(self, Self::Move { .. } | Self::LoadLocalPlain { .. } | Self::StoreLocalPlain { .. } | Self::Void { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Row = Operation<u64, ()>;

    #[test]
    fn a_row_names_its_registers_its_locals_and_whether_it_leaves() {
        let moved: Row = Operation::Move { destination: 1, source: 2 };
        assert_eq!(moved.registers(), Some([Some(1), Some(2)]));
        assert!(!moved.leaves() && !moved.uses_locals());
        let store: Row = Operation::StoreLocalPlain { source: 3, slot: 0, result: Some(4) };
        assert_eq!(store.registers(), Some([Some(3), Some(4)]));
        assert!(store.uses_locals() && !store.leaves());
        let this: Row = Operation::LoadThis { destination: 5 };
        assert_eq!(this.registers(), Some([Some(5), None]));
        assert!(this.leaves() && !this.uses_locals());
        let numeric: Row = Operation::Number(NumberOperation::ToNumeric { destination: 6, source: 7 });
        assert_eq!(numeric.registers(), Some([Some(6), Some(7)]));
        assert!(numeric.leaves());
        let step: Row = Operation::Number(NumberOperation::IncDec { destination: 8, source: 8, decrement: false,
            overflow: super::super::Overflow::Exit });
        assert_eq!(step.registers(), Some([Some(8), Some(8)]));
        let name: Row = Operation::Typeof { destination: 9, source: 10 };
        assert_eq!(name.registers(), Some([Some(9), Some(10)]));
        assert!(name.leaves() && !name.uses_locals());
        assert_eq!(Row::Jump.registers(), None);
    }
}
