//! Pure nested-loop and indexed-dataflow selection for dense matrix reductions.

use super::*;
use crate::{ir::Opcode, machine::CodeView};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Value {
    Local(u16),
    MatrixRow {
        matrix: u16,
        outer: u16,
    },
    MatrixElement {
        matrix: u16,
        outer: u16,
        inner: u16,
    },
    Product {
        left: Access,
        right: Access,
    },
    Updated {
        total: u16,
        left: Access,
        right: Access,
    },
}

#[derive(Clone, Copy)]
struct Access {
    matrix: u16,
    outer: u16,
    inner: u16,
}

pub(crate) fn select_function(code: CodeView<'_>) -> Option<FunctionMatrixReduction> {
    let mut initial_total = None;
    let mut selected = None;
    let mut returned = None;
    let mut constants = BTreeMap::new();
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        select_top(
            code,
            pc,
            instruction,
            &mut constants,
            &mut initial_total,
            &mut selected,
            &mut returned,
        )?;
    }
    let reduction = selected?;
    let (initial_slot, initial_total) = initial_total?;
    (initial_slot == reduction.total_slot).then_some(())?;
    (returned? == reduction.total_slot).then_some(())?;
    Some(FunctionMatrixReduction {
        reduction,
        initial_total,
    })
}

fn select_top(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    constants: &mut BTreeMap<u16, f64>,
    initial_total: &mut Option<(u16, f64)>,
    selected: &mut Option<MatrixReduction>,
    returned: &mut Option<u16>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadConst => track_constant(code, instruction, constants)?,
        Opcode::InitLocal if initial_total.is_none() => {
            *initial_total = Some((instruction.a, *constants.get(&instruction.b)?));
        }
        Opcode::LoadLocal | Opcode::LoadLocalChecked => *returned = Some(instruction.b),
        Opcode::Return => {}
        Opcode::Slow => select_top_slow(code, pc, selected)?,
        _ => return None,
    }
    Some(())
}

fn track_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    constants: &mut BTreeMap<u16, f64>,
) -> Option<()> {
    match code.constant(instruction.b)? {
        crate::ops::Constant::Number(value) => {
            constants.insert(instruction.a, *value);
        }
        crate::ops::Constant::Undefined => {
            constants.remove(&instruction.a);
        }
        _ => return None,
    }
    Some(())
}

fn select_top_slow(
    code: CodeView<'_>,
    pc: usize,
    selected: &mut Option<MatrixReduction>,
) -> Option<()> {
    match code.cold_at(pc)? {
        crate::ops::Op::MarkUninitialized { .. } | crate::ops::Op::MarkImmutable { .. } => Some(()),
        operation @ crate::ops::Op::Loop {
            label: None,
            post_test: false,
            ..
        } if selected.is_none() => {
            selected.replace(select_nested(operation)?);
            Some(())
        }
        _ => None,
    }
}

fn select_nested(operation: &crate::ops::Op) -> Option<MatrixReduction> {
    let mut loops = Vec::with_capacity(3);
    let update = select_level(operation, 0, &mut loops)?;
    let loops: [_; 3] = loops.try_into().ok()?;
    let [row, column, inner] = loops.map(|loop_| loop_.index_slot);
    let expected_left = (row, inner);
    let expected_right = (inner, column);
    ((update.left.outer, update.left.inner) == expected_left).then_some(())?;
    ((update.right.outer, update.right.inner) == expected_right).then_some(())?;
    Some(MatrixReduction {
        loops,
        total_slot: update.total,
        left_slot: update.left.matrix,
        right_slot: update.right.matrix,
    })
}

fn select_level(
    operation: &crate::ops::Op,
    depth: usize,
    loops: &mut Vec<crate::stencil_counted_loop::CountedLoop>,
) -> Option<Update> {
    let crate::ops::Op::Loop {
        init,
        test,
        body,
        update,
        per_iteration,
        ..
    } = operation
    else {
        return None;
    };
    let counted = crate::stencil_counted_loop::select(init.code()?, test.code()?, update.code()?)?;
    if per_iteration.as_slice() != [counted.index_slot] {
        return None;
    }
    loops.push(counted);
    if depth == 2 {
        return select_terminal(body.code()?);
    }
    select_nested_body(body.code()?, depth + 1, loops)
}

fn select_nested_body(
    code: CodeView<'_>,
    depth: usize,
    loops: &mut Vec<crate::stencil_counted_loop::CountedLoop>,
) -> Option<Update> {
    let mut selected = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadConst
                if matches!(
                    code.constant(instruction.b),
                    Some(crate::ops::Constant::Undefined)
                ) => {}
            Opcode::Move => {}
            Opcode::Slow if selected.is_none() => {
                selected = Some(select_level(code.cold_at(pc)?, depth, loops)?)
            }
            _ => return None,
        }
    }
    selected
}

#[derive(Clone, Copy)]
struct Update {
    total: u16,
    left: Access,
    right: Access,
}

fn select_terminal(code: CodeView<'_>) -> Option<Update> {
    let mut values = BTreeMap::new();
    let mut update = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        terminal_instruction(code, pc, instruction, &mut values, &mut update)?;
    }
    update
}

fn terminal_instruction(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
    update: &mut Option<Update>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadLocal | Opcode::LoadLocalChecked => {
            values.insert(instruction.a, Value::Local(instruction.b));
        }
        Opcode::AGetI => indexed_load(instruction, values)?,
        Opcode::Mul => multiply(instruction, values)?,
        Opcode::Add => add(instruction, values)?,
        Opcode::StoreLocal => store(instruction, values, update)?,
        Opcode::Move => copy(instruction, values)?,
        Opcode::Slow if admissible_binding_boundary(code.cold_at(pc)?) => {}
        _ => return None,
    }
    Some(())
}

fn indexed_load(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let base = *values.get(&instruction.b)?;
    let Value::Local(index) = values.get(&instruction.c)? else {
        return None;
    };
    let value = match base {
        Value::Local(matrix) => Value::MatrixRow {
            matrix,
            outer: *index,
        },
        Value::MatrixRow { matrix, outer } => Value::MatrixElement {
            matrix,
            outer,
            inner: *index,
        },
        _ => return None,
    };
    values.insert(instruction.a, value);
    Some(())
}

fn multiply(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Value>) -> Option<()> {
    let left = access(*values.get(&instruction.b)?)?;
    let right = access(*values.get(&instruction.c)?)?;
    values.insert(instruction.a, Value::Product { left, right });
    Some(())
}

fn add(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Value>) -> Option<()> {
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    let (total, left, right) = match (left, right) {
        (Value::Local(total), Value::Product { left, right })
        | (Value::Product { left, right }, Value::Local(total)) => (total, left, right),
        _ => return None,
    };
    values.insert(instruction.a, Value::Updated { total, left, right });
    Some(())
}

fn store(
    instruction: crate::ir::Instruction,
    values: &BTreeMap<u16, Value>,
    update: &mut Option<Update>,
) -> Option<()> {
    let Value::Updated { total, left, right } = values.get(&instruction.b)? else {
        return None;
    };
    (*total == instruction.a && update.is_none()).then_some(())?;
    *update = Some(Update {
        total: *total,
        left: *left,
        right: *right,
    });
    Some(())
}

fn access(value: Value) -> Option<Access> {
    let Value::MatrixElement {
        matrix,
        outer,
        inner,
    } = value
    else {
        return None;
    };
    Some(Access {
        matrix,
        outer,
        inner,
    })
}

fn copy(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Value>) -> Option<()> {
    values.insert(instruction.a, *values.get(&instruction.b)?);
    Some(())
}

fn admissible_binding_boundary(operation: &crate::ops::Op) -> bool {
    matches!(
        operation,
        crate::ops::Op::LoadBinding { dynamic: false, .. }
            | crate::ops::Op::CheckInitialized { .. }
            | crate::ops::Op::RequireObjectCoercible { .. }
            | crate::ops::Op::MarkImmutable { .. }
            | crate::ops::Op::MarkUninitialized { .. }
    )
}
