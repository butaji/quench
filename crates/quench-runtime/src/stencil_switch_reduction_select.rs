//! Pure residual-dataflow selection for the bounded switch-reduction family.

use super::*;
use crate::{ir::Opcode, machine::CodeView};
use std::collections::{BTreeMap, BTreeSet};

#[path = "stencil_switch_reduction_actions.rs"]
mod actions;

#[derive(Clone, Copy)]
enum Value {
    Other,
    Local(u16),
    Constant(i32),
    AffineIndex { sign: i32, bias: i32 },
    Selector { sign: i32, bias: i32, divisor: i32 },
    IndexMasked(i32),
    ShiftedTotal { slot: u16, count: i32 },
    Action { slot: u16, action: Action },
}

pub(crate) fn select_function(code: CodeView<'_>) -> Option<FunctionSwitchReduction> {
    let mut values = BTreeMap::new();
    let mut initial = BTreeMap::new();
    let mut reduction = None;
    let mut returned = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        select_top(
            code,
            pc,
            instruction,
            &mut values,
            &mut initial,
            &mut reduction,
            &mut returned,
        )?;
    }
    let reduction = reduction?;
    (returned? == reduction.total_slot).then_some(())?;
    let initial_total = *initial.get(&reduction.total_slot)?;
    Some(FunctionSwitchReduction {
        reduction,
        initial_total,
    })
}

fn select_top(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
    initial: &mut BTreeMap<u16, i64>,
    reduction: &mut Option<SwitchReduction>,
    returned: &mut Option<u16>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadConst => load_constant(code, instruction, values, true)?,
        Opcode::InitLocal => initialize(instruction, values, initial)?,
        Opcode::LoadLocal | Opcode::LoadLocalChecked => {
            values.insert(instruction.a, Value::Local(instruction.b));
        }
        Opcode::Binary => select_final_binary(instruction, values)?,
        Opcode::Return => select_return(instruction, values, returned),
        Opcode::Slow => select_top_slow(code, pc, reduction)?,
        _ => return None,
    }
    Some(())
}

fn initialize(
    instruction: crate::ir::Instruction,
    values: &BTreeMap<u16, Value>,
    initial: &mut BTreeMap<u16, i64>,
) -> Option<()> {
    let Value::Constant(value) = values.get(&instruction.b)? else {
        return None;
    };
    initial.insert(instruction.a, i64::from(*value));
    Some(())
}

fn select_final_binary(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let operator = crate::ir::compact_binary_operator(instruction.flags)?;
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    let slot = match (operator, left, right) {
        (crate::ops::BinaryOp::BitwiseOr, Value::Local(slot), Value::Constant(0))
        | (crate::ops::BinaryOp::BitwiseOr, Value::Constant(0), Value::Local(slot)) => slot,
        _ => return None,
    };
    values.insert(instruction.a, Value::Local(slot));
    Some(())
}

fn select_return(
    instruction: crate::ir::Instruction,
    values: &BTreeMap<u16, Value>,
    returned: &mut Option<u16>,
) {
    if let Some(Value::Local(slot)) = values.get(&instruction.a) {
        *returned = Some(*slot);
    }
}

fn select_top_slow(
    code: CodeView<'_>,
    pc: usize,
    selected: &mut Option<SwitchReduction>,
) -> Option<()> {
    match code.cold_at(pc)? {
        crate::ops::Op::MarkUninitialized { .. } | crate::ops::Op::MarkImmutable { .. } => Some(()),
        crate::ops::Op::Loop {
            label: None,
            post_test: false,
            init,
            test,
            body,
            update,
            per_iteration,
            ..
        } if selected.is_none() => {
            let counted =
                crate::stencil_counted_loop::select(init.code()?, test.code()?, update.code()?)?;
            (per_iteration.as_slice() == [counted.index_slot]).then_some(())?;
            selected.replace(select_loop_body(body.code()?, counted)?);
            Some(())
        }
        _ => None,
    }
}

fn select_loop_body(
    code: CodeView<'_>,
    counted: crate::stencil_counted_loop::CountedLoop,
) -> Option<SwitchReduction> {
    let mut values = BTreeMap::new();
    let mut switch = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadLocal | Opcode::LoadLocalChecked => {
                values.insert(instruction.a, Value::Local(instruction.b));
            }
            Opcode::LoadConst => load_constant(code, instruction, &mut values, false)?,
            Opcode::AddConst => add_constant(code, instruction, &mut values, counted.index_slot)?,
            Opcode::Add | Opcode::Sub => {
                affine_binary(instruction, &mut values, counted.index_slot)?
            }
            Opcode::Binary => selector_remainder(instruction, &mut values, counted.index_slot)?,
            Opcode::Move => copy(instruction, &mut values)?,
            Opcode::Slow if switch.is_none() => {
                let operation = code.cold_at(pc)?;
                switch = Some(select_switch(operation, &values, counted)?);
                let crate::ops::Op::Switch { dst, .. } = operation else {
                    return None;
                };
                values.insert(*dst, Value::Other);
            }
            _ => return None,
        }
    }
    switch
}

fn add_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
    index_slot: u16,
) -> Option<()> {
    let Value::Local(slot) = values.get(&instruction.b)? else {
        return None;
    };
    (*slot == index_slot).then_some(())?;
    let bias = exact_i32_constant(code, instruction.c)?;
    values.insert(instruction.a, Value::AffineIndex { sign: 1, bias });
    Some(())
}

fn affine_binary(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
    index_slot: u16,
) -> Option<()> {
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    let value = match (instruction.opcode, left, right) {
        (Opcode::Add, Value::Local(slot), Value::Constant(bias))
        | (Opcode::Add, Value::Constant(bias), Value::Local(slot))
            if slot == index_slot =>
        {
            Value::AffineIndex { sign: 1, bias }
        }
        (Opcode::Sub, Value::Local(slot), Value::Constant(value)) if slot == index_slot => {
            Value::AffineIndex {
                sign: 1,
                bias: value.checked_neg()?,
            }
        }
        (Opcode::Sub, Value::Constant(bias), Value::Local(slot)) if slot == index_slot => {
            Value::AffineIndex { sign: -1, bias }
        }
        _ => return None,
    };
    values.insert(instruction.a, value);
    Some(())
}

fn selector_remainder(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
    index_slot: u16,
) -> Option<()> {
    let operator = crate::ir::compact_binary_operator(instruction.flags)?;
    (operator == crate::ops::BinaryOp::Remainder).then_some(())?;
    let left = *values.get(&instruction.b)?;
    let Value::Constant(divisor) = values.get(&instruction.c)? else {
        return None;
    };
    let (sign, bias) = match left {
        Value::Local(slot) if slot == index_slot => (1, 0),
        Value::AffineIndex { sign, bias } => (sign, bias),
        _ => return None,
    };
    (*divisor > 0).then_some(())?;
    values.insert(
        instruction.a,
        Value::Selector {
            sign,
            bias,
            divisor: *divisor,
        },
    );
    Some(())
}

fn select_switch(
    operation: &crate::ops::Op,
    values: &BTreeMap<u16, Value>,
    counted: crate::stencil_counted_loop::CountedLoop,
) -> Option<SwitchReduction> {
    let crate::ops::Op::Switch {
        discriminant,
        cases,
        ..
    } = operation
    else {
        return None;
    };
    let Value::Selector {
        sign,
        bias,
        divisor,
    } = values.get(discriminant)?
    else {
        return None;
    };
    let (selected, default) = select_cases(cases, counted.index_slot)?;
    let total_slot = common_total_slot(&selected, default.0)?;
    Some(SwitchReduction {
        counted,
        total_slot,
        selector_sign: *sign,
        selector_bias: *bias,
        divisor: *divisor,
        cases: selected
            .into_iter()
            .map(|(value, _, action)| (value, action))
            .collect(),
        default: default.1,
    })
}

type SelectedCases = (Vec<(i32, u16, Action)>, (u16, Action));

fn select_cases(
    cases: &[(
        Option<crate::machine::FunctionCode>,
        crate::machine::FunctionCode,
    )],
    index_slot: u16,
) -> Option<SelectedCases> {
    (cases.len() <= MAX_CASES + 1).then_some(())?;
    let mut selected = Vec::new();
    let mut default = None;
    let mut seen = BTreeSet::new();
    for (index, (test, body)) in cases.iter().enumerate() {
        let (slot, action) = actions::select(body.code()?, index_slot)?;
        let is_last = index + 1 == cases.len();
        match test {
            Some(test) => {
                body_breaks(body.code()?).then_some(())?;
                let value = select_case_value(test.code()?)?;
                seen.insert(value).then_some(())?;
                selected.push((value, slot, action));
            }
            None if is_last && default.is_none() => default = Some((slot, action)),
            None => return None,
        }
    }
    Some((selected, default?))
}

fn select_case_value(code: CodeView<'_>) -> Option<i32> {
    let mut value = None;
    let mut returned = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadConst => {
                value = Some((instruction.a, exact_i32_constant(code, instruction.b)?))
            }
            Opcode::Return => returned = Some(instruction.a),
            _ => return None,
        }
    }
    let (register, value) = value?;
    (returned? == register).then_some(value)
}

fn load_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
    allow_undefined: bool,
) -> Option<()> {
    let value = match code.constant(instruction.b)? {
        crate::ops::Constant::Number(value) => {
            Value::Constant(crate::stencil_numeric_integer_selection::exact_i32(*value)?)
        }
        crate::ops::Constant::Undefined if allow_undefined => Value::Other,
        crate::ops::Constant::Undefined => Value::Other,
        _ => return None,
    };
    values.insert(instruction.a, value);
    Some(())
}

fn exact_i32_constant(code: CodeView<'_>, constant: u16) -> Option<i32> {
    let crate::ops::Constant::Number(value) = code.constant(constant)? else {
        return None;
    };
    crate::stencil_numeric_integer_selection::exact_i32(*value)
}

fn copy(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Value>) -> Option<()> {
    values.insert(instruction.a, *values.get(&instruction.b)?);
    Some(())
}

fn body_breaks(code: CodeView<'_>) -> bool {
    (0..code.len()).any(|pc| code.cold_at(pc).is_some_and(is_unlabelled_break))
}

fn is_unlabelled_break(operation: &crate::ops::Op) -> bool {
    matches!(
        operation,
        crate::ops::Op::Break {
            label: None,
            value: None
        }
    )
}

fn common_total_slot(cases: &[(i32, u16, Action)], default_slot: u16) -> Option<u16> {
    cases
        .iter()
        .all(|(_, slot, _)| *slot == default_slot)
        .then_some(default_slot)
}
