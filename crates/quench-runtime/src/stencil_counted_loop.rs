//! Canonical bounded facts for ordinary increasing counted loops.

use crate::{ir::Opcode, machine::CodeView};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CountedLoop {
    pub(crate) index_slot: u16,
    pub(crate) start: i32,
    pub(crate) end: i32,
}

pub(crate) fn select(
    init: CodeView<'_>,
    test: CodeView<'_>,
    update: CodeView<'_>,
) -> Option<CountedLoop> {
    let (index_slot, start) = select_init(init)?;
    let end = select_test(test, index_slot)?;
    select_update(update, index_slot)?;
    Some(CountedLoop {
        index_slot,
        start,
        end,
    })
}

fn select_init(code: CodeView<'_>) -> Option<(u16, i32)> {
    let mut constant = None;
    let mut initialized = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadConst => select_init_constant(code, instruction, &mut constant)?,
            Opcode::InitLocal => initialized = Some((instruction.a, instruction.b)),
            Opcode::Return => {}
            Opcode::Slow if is_uninitialized_marker(code, pc) => {}
            _ => return None,
        }
    }
    let (register, value) = constant?;
    let (slot, source) = initialized?;
    (register == source).then_some((slot, value))
}

fn select_init_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    selected: &mut Option<(u16, i32)>,
) -> Option<()> {
    match code.constant(instruction.b)? {
        crate::ops::Constant::Number(value) => {
            *selected = Some((instruction.a, exact_i32(*value)?));
        }
        crate::ops::Constant::Undefined => {}
        _ => return None,
    }
    Some(())
}

fn select_test(code: CodeView<'_>, index_slot: u16) -> Option<i32> {
    let mut index_register = None;
    let mut bound = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadLocal | Opcode::LoadLocalChecked if instruction.b == index_slot => {
                index_register = Some(instruction.a);
            }
            Opcode::LoadConst => bound = Some((instruction.a, number(code, instruction.b)?)),
            Opcode::Binary => select_less_than(instruction, index_register?, bound?.0)?,
            Opcode::Return => {}
            _ => return None,
        }
    }
    Some(bound?.1)
}

fn select_less_than(
    instruction: crate::ir::Instruction,
    index_register: u16,
    bound_register: u16,
) -> Option<()> {
    let operator = crate::ir::compact_binary_operator(instruction.flags)?;
    (operator == crate::ops::BinaryOp::LessThan
        && instruction.b == index_register
        && instruction.c == bound_register)
        .then_some(())
}

fn select_update(code: CodeView<'_>, index_slot: u16) -> Option<()> {
    let mut found = false;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::UpdateLocal if instruction.flags == 0 && instruction.c == index_slot => {
                found = true;
            }
            Opcode::Return => {}
            _ => return None,
        }
    }
    found.then_some(())
}

fn is_uninitialized_marker(code: CodeView<'_>, pc: usize) -> bool {
    matches!(
        code.cold_at(pc),
        Some(crate::ops::Op::MarkUninitialized { .. })
    )
}

fn number(code: CodeView<'_>, constant: u16) -> Option<i32> {
    let crate::ops::Constant::Number(value) = code.constant(constant)? else {
        return None;
    };
    exact_i32(*value)
}

fn exact_i32(value: f64) -> Option<i32> {
    crate::stencil_numeric_integer_selection::exact_i32(value)
}
