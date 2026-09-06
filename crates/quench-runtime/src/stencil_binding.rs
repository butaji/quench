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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalOutputValue {
    Array,
    Element,
    Index,
    Result,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalOutputDestination {
    Register(PhysicalOperand),
    LocalSlot(PhysicalOperand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalOutput {
    pub value: PhysicalOutputValue,
    pub destination: PhysicalOutputDestination,
}

impl PhysicalOperand {
    pub(crate) fn read(self, instructions: &[crate::ir::Instruction]) -> Option<u16> {
        operand_value(self, instructions)
    }
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

    pub(crate) fn outputs_cover_live_definitions(
        &self,
        entries: &[crate::machine::BaselineEntry],
        start: usize,
        live_at_exit: &std::collections::BTreeSet<u16>,
    ) -> bool {
        self.operations.iter().enumerate().all(|(offset, _)| {
            let definition = entries
                .get(start + offset)
                .and_then(|entry| entry.instruction.register_flow().definition);
            definition.is_none_or(|register| {
                !live_at_exit.contains(&register) || self.output_register(entries, start, register)
            })
        })
    }

    fn output_register(
        &self,
        entries: &[crate::machine::BaselineEntry],
        start: usize,
        expected: u16,
    ) -> bool {
        self.outputs.iter().any(|output| {
            let PhysicalOutputDestination::Register(operand) = output.destination else {
                return false;
            };
            entries
                .get(start + usize::from(operand.operation))
                .is_some_and(|entry| {
                    instruction_field(entry.instruction, operand.field) == expected
                })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(instruction: crate::ir::Instruction) -> crate::machine::BaselineEntry {
        crate::machine::BaselineEntry {
            instruction,
            handler: instruction.opcode.handler(),
            control: instruction.opcode.control_operands(instruction),
        }
    }

    #[test]
    fn declared_outputs_cover_only_their_exact_live_definitions() {
        static OUTPUTS: [PhysicalOutput; 1] = [PhysicalOutput {
            value: PhysicalOutputValue::Result,
            destination: PhysicalOutputDestination::Register(PhysicalOperand {
                operation: 0,
                field: PhysicalOperandField::A,
            }),
        }];
        let mut record = crate::stencil_select::RegionRecord {
            name: "output_test",
            key: crate::stencil_fact::RegionKey(19),
            stencil: crate::stencil_fact::Stencil {
                bytes: &[],
                holes: &[],
            },
            operations: &[crate::ir::Opcode::Move],
            bindings: &[],
            outputs: &OUTPUTS,
            entry: 0,
            external_entries: &[0],
            fallthrough: None,
            abi: crate::stencil_select::RegionAbi::ArrayNumericLoop,
            template_calls_helper: false,
            executable: false,
        };
        let entries = [entry(crate::ir::Instruction::move_(2, 1))];
        assert!(record.outputs_cover_live_definitions(
            &entries,
            0,
            &std::collections::BTreeSet::from([2])
        ));
        record.outputs = &[];
        assert!(!record.outputs_cover_live_definitions(
            &entries,
            0,
            &std::collections::BTreeSet::from([2])
        ));
    }
}
