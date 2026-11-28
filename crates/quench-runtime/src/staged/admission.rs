//! What a tier asks of an operation before it steps it, as data on the operation: the registers
//! it names, whether it reads the activation's locals, and whether it can leave at the frame's
//! miss edge. A tier decides by these, never by the opcode the operation decoded from.

use super::{NumberOperation, Operation, Register};
use crate::bytecode::{InstructionField, Op, Traits, WideInstruction};
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

impl<W, P> Operation<W, P> {
    /// The registers of a row over registers, locals and `this` (at most two), which the tier
    /// must hold in its window to step it; `None` for any other row.
    pub const fn registers(&self) -> Option<[Option<Register>; 2]> {
        Some(match *self {
            Self::Move { destination, source } | Self::LogicalNot { destination, source }
                | Self::Number(NumberOperation::ToNumeric { destination, source } | NumberOperation::IncDec { destination, source, .. }) =>
                [Some(destination), Some(source)],
            Self::LoadLocalPlain { destination, .. } | Self::LoadLocalChecked { destination, .. } | Self::LoadThis { destination }
                | Self::Void { destination } => [Some(destination), None],
            Self::StoreLocalPlain { source, result, .. } | Self::StoreLocalChecked { source, result, .. } => [Some(source), result],
            Self::InitializeLocal { .. } => [None, None],
            Self::RequireObjectCoercible { register } => [Some(register), None],
            _ => return None,
        })
    }

    /// Whether the row reads or writes the activation's local slots (`Frame::load_local`), which a
    /// numeric region's activation has none of.
    pub const fn uses_locals(&self) -> bool {
        matches!(self, Self::LoadLocalPlain { .. } | Self::StoreLocalPlain { .. } | Self::LoadLocalChecked { .. }
            | Self::StoreLocalChecked { .. } | Self::InitializeLocal { .. })
    }

    /// Whether a frame's step of the row can leave at its miss edge: a row that only copies or
    /// defines a constant word never does.
    pub const fn leaves(&self) -> bool {
        !matches!(self, Self::Move { .. } | Self::LoadLocalPlain { .. } | Self::StoreLocalPlain { .. } | Self::Void { .. })
    }
}

impl Op {
    /// An allocation the lane makes in place, running no guest code: a literal (`compiled_new_object`,
    /// `new_object_pair_in_lane`, `new_array_in_lane`) or a construction by its callee's plan
    /// (`construct_in_lane`). A due collection leaves before it, so native code never collects.
    pub const fn allocates(self) -> bool {
        self.traits().has(Traits::ALLOCATES)
    }

    /// An instruction whose result register a loop region defines natively, as a Number or as a
    /// tagged value.
    pub const fn defines_loop_result(self) -> bool {
        self.allocates() || self.traits().has(Traits::DEFINES_LOOP_RESULT)
    }

    /// An instruction a loop region may lower to native code; any other leaves at its exact PC.
    /// A conditional jump on two values is further bounded by its operator
    /// (`osr_loop::lowered_natively`).
    pub const fn lowers_in_loop(self) -> bool {
        self.defines_loop_result() || self.is_truthiness_jump() || self.traits().has(Traits::LOWERS_IN_LOOP)
    }

    /// An instruction whose result may hold any boxed value: an allocation, a copy, or a read of
    /// a capture, a field, an element, an environment local or a global.
    pub const fn carries_boxed_value(self) -> bool {
        self.allocates() || self.traits().has(Traits::CARRIES_BOXED_VALUE)
    }

    /// An instruction whose register and operand fields a loop region reads as values, the
    /// receiver of an indexed access aside (`WideInstruction::value_registers`).
    pub const fn reads_values(self) -> bool {
        self.is_truthiness_jump() || self.is_binary_arithmetic() || self.traits().has(Traits::READS_VALUES)
    }
}

impl WideInstruction {
    /// The register `field` reads as a value: a register field, or an operand field naming a
    /// register; `None` for a constant operand and for any field that is not read.
    fn read_register(self, field: InstructionField) -> Option<Register> {
        let layout = self.op().field_layout(field);
        match field {
            InstructionField::B if layout.is_operand_field() => self.operand_b().register_index(),
            InstructionField::C if layout.is_operand_field() => self.operand_c().register_index(),
            _ => layout.reads_register().then(|| self.field_value(field)),
        }
    }

    /// The registers a value-reading instruction (`Op::reads_values`) reads as values, in field
    /// order; an indexed access's receiver (field B) is a heap view, not a value.
    pub fn value_registers(self) -> impl Iterator<Item = Register> {
        let receiver = self.op().is_indexed_access();
        InstructionField::ALL.iter().copied()
            .filter(move |field| !(receiver && *field == InstructionField::B))
            .filter_map(move |field| self.read_register(field))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Operand;

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
        assert_eq!(Row::Jump.registers(), None);
    }

    /// Every opcode's loop-admission metadata, against the opcode families spelled out once
    /// here: a table over the whole catalog, so a new opcode answers by its family or not at all.
    #[test]
    fn every_opcode_answers_the_loop_admission_questions_by_its_family() {
        use crate::bytecode::Class::{self, *};
        let allocations: &[Class] = &[MakeObject, MakeObject2, MakeObjectLiteral, MakeArray, Construct];
        let defining: &[Class] = &[LoadConst, Move, LoadCapture, Binary, NumericAdd, NumericMultiply, IncDec, Unary,
            ToNumeric, GetIndex, GetField, LoadEnvLocal, LoadName];
        let lowered: &[Class] = &[StoreCapture, RequireObjectCoercible, SetIndex, DefineArrayElement, Return, Jump,
            JumpFalse, JumpTrue, IntCaseIndex, WasmBranchTable, LoadThis];
        let reading: &[Class] = &[Move, IncDec, Unary, ToNumeric, Binary, NumericAdd, NumericMultiply,
            JumpBinaryFalse, JumpFalse, JumpTrue, GetIndex, SetIndex];
        let boxed: &[Class] = &[GetIndex, GetField, Move, LoadCapture, LoadEnvLocal, LoadName, LoadThis];
        for index in 0..Op::COUNT {
            let op = Op::from_index(index).expect("every catalog index is an opcode");
            let class = op.class();
            let allocates = allocations.contains(&class);
            let defines = allocates || defining.contains(&class);
            assert_eq!(op.allocates(), allocates, "{op:?}");
            assert_eq!(op.defines_loop_result(), defines, "{op:?}");
            assert_eq!(op.lowers_in_loop(), defines || lowered.contains(&class), "{op:?}");
            assert_eq!(op.reads_values(), reading.contains(&class), "{op:?}");
            assert_eq!(op.carries_boxed_value(), allocates || boxed.contains(&class), "{op:?}");
        }
        assert!(!Op::JumpBinaryFalse.lowers_in_loop(), "a two-value jump is bounded by its operator");
    }

    /// The registers a value-reading instruction reads come from its field layouts: register
    /// fields and register operands, never an indexed access's receiver or a constant.
    #[test]
    fn value_registers_follow_the_field_layouts() {
        let reads = |instruction: WideInstruction| instruction.value_registers().collect::<Vec<_>>();
        let register = |index| Operand::register(index).0;
        let constant = Operand::constant(3).expect("a small pool index fits").0;
        assert_eq!(reads(WideInstruction::new(Op::Move, 1, 2, 0, 0)), [2]);
        assert_eq!(reads(WideInstruction::new(Op::JumpFalse, 6, 0, 0, 0)), [6]);
        assert_eq!(reads(WideInstruction::new(Op::SetIndex, 3, 4, 5, 0)), [3, 5]);
        assert_eq!(reads(WideInstruction::new(Op::GetIndex, 1, register(2), register(7), 0)), [7]);
        assert_eq!(reads(WideInstruction::new(Op::NumericAdd, 1, register(8), register(9), 0)), [8, 9]);
        assert_eq!(reads(WideInstruction::new(Op::NumericAdd, 1, register(8), constant, 0)), [8]);
    }
}
