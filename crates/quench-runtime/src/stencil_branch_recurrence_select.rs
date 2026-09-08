use super::*;
use crate::{ir::Opcode, machine::CodeView};
use std::collections::BTreeMap;

#[derive(Clone)]
enum Numeric {
    Other,
    Local(u16),
    Constant(f64),
    Product {
        state_slot: u16,
        multiplier: f64,
    },
    Affine {
        state_slot: u16,
        multiplier: f64,
        addend: f64,
    },
    Uint32 {
        state_slot: u16,
        multiplier: f64,
        addend: f64,
    },
    MaskedState {
        state_slot: u16,
        mask: u32,
    },
    Predicate {
        state_slot: u16,
        mask: u32,
        expected: u32,
        invert: bool,
    },
}

#[path = "stencil_branch_recurrence_arms.rs"]
mod arms;

pub(crate) fn select_function(code: CodeView<'_>) -> Option<FunctionRecurrence> {
    let facts = crate::stencil_counted_function::select(code, |counted, body, per_iteration| {
        (per_iteration == [counted.index_slot]).then_some(())?;
        select_body(body, counted)
    })?;
    let recurrence = facts.loop_cover;
    (facts.returned.slot == recurrence.score_slot
        && facts.returned.representation
            == crate::stencil_counted_function::ReturnedRepresentation::Direct)
        .then_some(())?;
    let initial_score =
        crate::stencil_counted_function::initial_i64(&facts.initials, recurrence.score_slot)?;
    let initial_state = initialized_u32(&facts.initials, recurrence.state_slot)?;
    Some(FunctionRecurrence {
        recurrence,
        initial_score,
        initial_state,
    })
}

fn initialized_u32(
    locals: &[crate::stencil_counted_function::InitialNumber],
    slot: u16,
) -> Option<u32> {
    let value = crate::stencil_counted_function::initial_i64(locals, slot)?;
    u32::try_from(value).ok()
}

fn select_body(
    code: CodeView<'_>,
    counted: crate::stencil_counted_loop::CountedLoop,
) -> Option<Recurrence> {
    let mut values = BTreeMap::new();
    let mut state = None;
    let mut predicate = None;
    let mut branch = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        if instruction.opcode == Opcode::JumpIfFalse {
            let Numeric::Predicate {
                state_slot,
                mask,
                expected,
                invert,
            } = values.get(&instruction.a)?
            else {
                return None;
            };
            predicate = Some((*state_slot, *mask, *expected, *invert));
            branch = Some((pc, instruction.a));
            break;
        }
        select_prefix(code, instruction, &mut values, &mut state)?;
    }
    let (state_slot, multiplier, addend) = state?;
    let (predicate_state, predicate_mask, predicate_expected, predicate_invert) = predicate?;
    let (pc, condition_register) = branch?;
    let (score_slot, when_true) =
        arms::select(code, pc, condition_register, counted.index_slot, true)?;
    let (false_slot, when_false) =
        arms::select(code, pc, condition_register, counted.index_slot, false)?;
    (predicate_state == state_slot
        && score_slot == false_slot
        && score_slot != state_slot
        && score_slot != counted.index_slot)
        .then_some(())?;
    Some(Recurrence {
        score_slot,
        state_slot,
        start: counted.start,
        end: counted.end,
        multiplier,
        addend,
        predicate_mask,
        predicate_expected,
        predicate_invert,
        when_true,
        when_false,
    })
}

fn select_prefix(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Numeric>,
    state: &mut Option<(u16, f64, f64)>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadLocal | Opcode::LoadLocalChecked => {
            values.insert(instruction.a, Numeric::Local(instruction.b));
        }
        Opcode::LoadConst => {
            let value = match code.constant(instruction.b)? {
                crate::ops::Constant::Number(value) => Numeric::Constant(*value),
                crate::ops::Constant::Undefined => Numeric::Other,
                _ => return None,
            };
            values.insert(instruction.a, value);
        }
        Opcode::Mul => multiply(instruction, values)?,
        Opcode::Add => add(instruction, values)?,
        Opcode::AddConst => add_constant(code, instruction, values)?,
        Opcode::Binary => binary(instruction, values)?,
        Opcode::StoreLocal => store_state(instruction, values, state)?,
        Opcode::Move => {
            values.insert(instruction.a, values.get(&instruction.b)?.clone());
        }
        _ => return None,
    }
    Some(())
}

fn add(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Numeric>) -> Option<()> {
    let left = values.get(&instruction.b)?;
    let right = values.get(&instruction.c)?;
    let (product, addend) = match (left, right) {
        (
            Numeric::Product {
                state_slot,
                multiplier,
            },
            Numeric::Constant(addend),
        )
        | (
            Numeric::Constant(addend),
            Numeric::Product {
                state_slot,
                multiplier,
            },
        ) => ((*state_slot, *multiplier), *addend),
        _ => return None,
    };
    values.insert(
        instruction.a,
        Numeric::Affine {
            state_slot: product.0,
            multiplier: product.1,
            addend,
        },
    );
    Some(())
}

fn multiply(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Numeric>,
) -> Option<()> {
    let left = values.get(&instruction.b)?;
    let right = values.get(&instruction.c)?;
    let (state_slot, multiplier) = match (left, right) {
        (Numeric::Local(slot), Numeric::Constant(value))
        | (Numeric::Constant(value), Numeric::Local(slot)) => (*slot, *value),
        _ => return None,
    };
    values.insert(
        instruction.a,
        Numeric::Product {
            state_slot,
            multiplier,
        },
    );
    Some(())
}

fn add_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Numeric>,
) -> Option<()> {
    let Numeric::Product {
        state_slot,
        multiplier,
    } = values.get(&instruction.b)?
    else {
        return None;
    };
    let addend = number(code, instruction.c)?;
    values.insert(
        instruction.a,
        Numeric::Affine {
            state_slot: *state_slot,
            multiplier: *multiplier,
            addend,
        },
    );
    Some(())
}

fn binary(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Numeric>) -> Option<()> {
    let operator = crate::ir::compact_binary_operator(instruction.flags)?;
    let left = values.get(&instruction.b)?.clone();
    let right = values.get(&instruction.c)?.clone();
    let value = match (operator, left, right) {
        (
            crate::ops::BinaryOp::ShiftRightZeroFill,
            Numeric::Affine {
                state_slot,
                multiplier,
                addend,
            },
            Numeric::Constant(value),
        ) if value == 0.0 => Numeric::Uint32 {
            state_slot,
            multiplier,
            addend,
        },
        (crate::ops::BinaryOp::BitwiseAnd, left, right) => masked_state(left, right)?,
        (crate::ops::BinaryOp::StrictEqual, left, right) => predicate(left, right, false)?,
        (crate::ops::BinaryOp::StrictNotEqual, left, right) => predicate(left, right, true)?,
        _ => return None,
    };
    values.insert(instruction.a, value);
    Some(())
}

fn masked_state(left: Numeric, right: Numeric) -> Option<Numeric> {
    let (state_slot, mask) = match (left, right) {
        (Numeric::Local(slot), Numeric::Constant(mask))
        | (Numeric::Constant(mask), Numeric::Local(slot)) => (slot, exact_u32(mask)?),
        _ => return None,
    };
    Some(Numeric::MaskedState { state_slot, mask })
}

fn predicate(left: Numeric, right: Numeric, invert: bool) -> Option<Numeric> {
    let (state_slot, mask, expected) = match (left, right) {
        (Numeric::MaskedState { state_slot, mask }, Numeric::Constant(expected))
        | (Numeric::Constant(expected), Numeric::MaskedState { state_slot, mask }) => {
            (state_slot, mask, exact_u32(expected)?)
        }
        _ => return None,
    };
    Some(Numeric::Predicate {
        state_slot,
        mask,
        expected,
        invert,
    })
}

fn store_state(
    instruction: crate::ir::Instruction,
    values: &BTreeMap<u16, Numeric>,
    selected: &mut Option<(u16, f64, f64)>,
) -> Option<()> {
    let Numeric::Uint32 {
        state_slot,
        multiplier,
        addend,
    } = values.get(&instruction.b)?
    else {
        return None;
    };
    (*state_slot == instruction.a && selected.is_none()).then_some(())?;
    *selected = Some((*state_slot, *multiplier, *addend));
    Some(())
}

fn number(code: CodeView<'_>, constant: u16) -> Option<f64> {
    let crate::ops::Constant::Number(value) = code.constant(constant)? else {
        return None;
    };
    Some(*value)
}

fn exact_u32(value: f64) -> Option<u32> {
    (value.is_finite() && value.fract() == 0.0 && value >= 0.0 && value <= u32::MAX as f64)
        .then_some(value as u32)
}
