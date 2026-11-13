//! A read-only dataflow view of a program's bytecode for offline tools (supernode mining):
//! per instruction, its opcode name, the registers it writes and reads, and its ordinary
//! successors, all derived from the opcode field layouts.
use super::instruction::WideInstruction;
use super::{FieldLayout, ImmediateRole, InstructionField, Operand, ResidualProgram, control_flow};
use oxc_ast::ast::BinaryOperator;

/// Every `BinaryOperator`, to name an operator by its discriminant.
const BINARY_OPERATORS: [BinaryOperator; 22] = [
    BinaryOperator::Equality, BinaryOperator::Inequality, BinaryOperator::StrictEquality,
    BinaryOperator::StrictInequality, BinaryOperator::LessThan, BinaryOperator::LessEqualThan,
    BinaryOperator::GreaterThan, BinaryOperator::GreaterEqualThan, BinaryOperator::Addition,
    BinaryOperator::Subtraction, BinaryOperator::Multiplication, BinaryOperator::Division,
    BinaryOperator::Remainder, BinaryOperator::Exponential, BinaryOperator::ShiftLeft,
    BinaryOperator::ShiftRight, BinaryOperator::ShiftRightZeroFill, BinaryOperator::BitwiseOR,
    BinaryOperator::BitwiseXOR, BinaryOperator::BitwiseAnd, BinaryOperator::In,
    BinaryOperator::Instanceof,
];

#[derive(Clone, Debug)]
pub struct FlowInstruction {
    pub opcode: u16,
    pub name: &'static str,
    /// The JavaScript operator symbol of a binary-operator instruction (`+`, `<`, ...).
    pub operator: Option<&'static str>,
    /// Registers written (`WideInstruction::written_registers`).
    pub defs: Vec<u16>,
    /// Registers read: register fields and register-kind operands.
    pub uses: Vec<u16>,
    /// Ordinary successor PCs (`for_each_instruction_successor`).
    pub successors: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct FlowFunction {
    pub name: Option<String>,
    pub instructions: Vec<FlowInstruction>,
    /// Exception handler entry PCs, which start basic blocks.
    pub handler_targets: Vec<u32>,
}

fn used_registers(instruction: WideInstruction) -> Vec<u16> {
    InstructionField::ALL
        .iter()
        .copied()
        .filter_map(|field| {
            let layout = instruction.op().field_layout(field);
            let value = instruction.field_value(field);
            if layout.reads_register() {
                Some(value)
            } else if layout.is_operand_field() {
                Operand(value).register_index()
            } else {
                None
            }
        })
        .collect()
}

fn operator_discriminant(instruction: WideInstruction) -> Option<u32> {
    let op = instruction.op();
    if matches!(
        op.immediate_role(),
        ImmediateRole::BinaryOperator | ImmediateRole::AdditionOperator | ImmediateRole::MultiplicationOperator
    ) {
        return Some(instruction.binary_operator());
    }
    InstructionField::ALL
        .iter()
        .find(|&&field| op.field_layout(field) == FieldLayout::BinaryOperator)
        .map(|&field| u32::from(instruction.field_value(field)))
}

fn operator_symbol(instruction: WideInstruction) -> Option<&'static str> {
    let discriminant = operator_discriminant(instruction)?;
    BINARY_OPERATORS.iter().find(|operator| **operator as u32 == discriminant).map(|operator| operator.as_str())
}

fn flow_instruction(pc: usize, instruction: WideInstruction) -> FlowInstruction {
    let mut successors = Vec::new();
    control_flow::for_each_instruction_successor(pc, instruction, |next| successors.push(next as u32));
    let op = instruction.op();
    FlowInstruction {
        opcode: op as u16,
        name: super::Op::NAMES[op as usize],
        operator: operator_symbol(instruction),
        defs: instruction.written_registers().collect(),
        uses: used_registers(instruction),
        successors,
    }
}

impl ResidualProgram {
    /// Every function's instructions in the dataflow view.
    pub fn flow_functions(&self) -> Vec<FlowFunction> {
        self.compile_released();
        self.functions
            .iter()
            .map(|function| FlowFunction {
                name: function.name.map(|atom| {
                    let text: &str = self.atoms[atom as usize].as_ref();
                    text.to_owned()
                }),
                instructions: function
                    .instructions()
                    .enumerate()
                    .map(|(pc, instruction)| flow_instruction(pc, instruction))
                    .collect(),
                handler_targets: function.handlers.iter().map(|handler| handler.target).collect(),
            })
            .collect()
    }
}
