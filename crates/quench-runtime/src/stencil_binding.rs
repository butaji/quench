//! Physical operand wiring derived from canonical stencil recipes.
//!
//! These facts constrain register/target relationships only. JavaScript
//! meaning, effects, and operand roles remain owned by the canonical IR.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalOperandField {
    A,
    B,
    C,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalOperand {
    pub operation: u8,
    pub field: PhysicalOperandField,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalBindingValue {
    Operand(PhysicalOperand),
    RegionStart,
    RegionEnd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalBinding {
    Equal(PhysicalBindingValue, PhysicalBindingValue),
    AllDistinct(&'static [PhysicalOperand]),
}

impl crate::stencil_select::RegionRecord {
    pub fn bindings_match(&self, instructions: &[crate::ir::Instruction], start: usize) -> bool {
        self.bindings_match_with(instructions.len(), start, |operation, field| {
            operand_value(
                PhysicalOperand {
                    operation: operation as u8,
                    field,
                },
                instructions,
            )
        })
    }

    pub(crate) fn bindings_match_entries(
        &self,
        entries: &[crate::machine::BaselineEntry],
        start: usize,
    ) -> bool {
        self.bindings_match_with(self.operations.len(), start, |operation, field| {
            let instruction = entries.get(start + operation)?.instruction;
            Some(instruction_field(instruction, field))
        })
    }

    fn bindings_match_with(
        &self,
        span_len: usize,
        start: usize,
        mut operand: impl FnMut(usize, PhysicalOperandField) -> Option<u16>,
    ) -> bool {
        self.bindings
            .iter()
            .all(|binding| binding.matches(span_len, start, &mut operand))
    }
}

impl PhysicalBinding {
    fn matches(
        self,
        span_len: usize,
        start: usize,
        operand: &mut impl FnMut(usize, PhysicalOperandField) -> Option<u16>,
    ) -> bool {
        match self {
            Self::Equal(left, right) => match (
                binding_value(left, span_len, start, operand),
                binding_value(right, span_len, start, operand),
            ) {
                (Some(left), Some(right)) => left == right,
                _ => false,
            },
            Self::AllDistinct(inputs) => distinct(inputs, operand),
        }
    }
}

fn distinct(
    inputs: &[PhysicalOperand],
    operand: &mut impl FnMut(usize, PhysicalOperandField) -> Option<u16>,
) -> bool {
    inputs.iter().enumerate().all(|(index, input)| {
        let Some(value) = operand(usize::from(input.operation), input.field) else {
            return false;
        };
        inputs[index + 1..]
            .iter()
            .all(|other| operand(usize::from(other.operation), other.field) != Some(value))
    })
}

fn binding_value(
    value: PhysicalBindingValue,
    span_len: usize,
    start: usize,
    operand: &mut impl FnMut(usize, PhysicalOperandField) -> Option<u16>,
) -> Option<u16> {
    match value {
        PhysicalBindingValue::Operand(input) => operand(usize::from(input.operation), input.field),
        PhysicalBindingValue::RegionStart => u16::try_from(start).ok(),
        PhysicalBindingValue::RegionEnd => u16::try_from(start.checked_add(span_len)?).ok(),
    }
}

fn operand_value(operand: PhysicalOperand, instructions: &[crate::ir::Instruction]) -> Option<u16> {
    let instruction = instructions.get(usize::from(operand.operation))?;
    Some(instruction_field(*instruction, operand.field))
}

const fn instruction_field(
    instruction: crate::ir::Instruction,
    field: PhysicalOperandField,
) -> u16 {
    match field {
        PhysicalOperandField::A => instruction.a,
        PhysicalOperandField::B => instruction.b,
        PhysicalOperandField::C => instruction.c,
    }
}
