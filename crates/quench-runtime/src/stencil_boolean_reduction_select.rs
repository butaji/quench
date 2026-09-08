use super::*;
use std::collections::BTreeMap;

pub(super) fn select(
    init: CodeView<'_>,
    test: CodeView<'_>,
    body: CodeView<'_>,
    update: CodeView<'_>,
    per_iteration: &[u16],
) -> Option<Reduction> {
    let (index_slot, start) = select_init(init)?;
    let end = select_test(test, index_slot)?;
    select_update(update, index_slot)?;
    let (count_slot, left_slot, left, right_slot, right, truth_table) =
        select_body(body, index_slot)?;
    iteration_slots_match(per_iteration, index_slot, left_slot, right_slot)?;
    Some(Reduction {
        index_slot,
        count_slot,
        start,
        end,
        left,
        right,
        truth_table,
    })
}

pub(crate) fn select_function(code: CodeView<'_>) -> Option<FunctionReduction> {
    let mut initial = None;
    let mut initialized = None;
    let mut loaded_count = None;
    let mut reduction = None;
    let mut returned_count = false;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadConst => select_function_constant(code, instruction, &mut initial)?,
            Opcode::InitLocal if initialized.is_none() => {
                initialized = Some((instruction.a, instruction.b));
            }
            Opcode::LoadLocal | Opcode::LoadLocalChecked => {
                if loaded_count.is_some_and(|(_, slot)| slot != instruction.b) {
                    return None;
                }
                loaded_count = Some((instruction.a, instruction.b));
            }
            Opcode::Slow => select_function_slow(code, pc, &mut reduction)?,
            Opcode::Return => {
                returned_count |=
                    loaded_count.is_some_and(|(register, _)| register == instruction.a);
            }
            _ => return None,
        }
    }
    let (register, initial_count) = initial?;
    let (count_slot, source) = initialized?;
    (register == source && returned_count && loaded_count?.1 == count_slot).then_some(())?;
    let reduction = reduction?;
    (reduction.count_slot == count_slot).then_some(FunctionReduction {
        reduction,
        initial_count,
    })
}

fn select_function_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    initial: &mut Option<(u16, i32)>,
) -> Option<()> {
    match code.constant(instruction.b)? {
        crate::ops::Constant::Number(value) => {
            initial.is_none().then_some(())?;
            *initial = Some((
                instruction.a,
                crate::stencil_numeric_integer_selection::exact_i32(*value)?,
            ));
        }
        crate::ops::Constant::Undefined => {}
        _ => return None,
    }
    Some(())
}

fn select_function_slow(
    code: CodeView<'_>,
    pc: usize,
    selected: &mut Option<Reduction>,
) -> Option<()> {
    match code.cold_at(pc)? {
        crate::ops::Op::MarkUninitialized { .. } | crate::ops::Op::MarkImmutable { .. } => Some(()),
        crate::ops::Op::Loop {
            label,
            init,
            test,
            body,
            update,
            post_test,
            per_iteration,
            ..
        } if label.is_none() && !post_test => {
            selected.is_none().then_some(())?;
            let value = select(
                init.code()?,
                test.code()?,
                body.code()?,
                update.code()?,
                per_iteration,
            )?;
            selected.replace(value);
            Some(())
        }
        _ => None,
    }
}

fn select_init(code: CodeView<'_>) -> Option<(u16, i32)> {
    let mut constant = None;
    let mut initialized = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadConst => match code.constant(instruction.b)? {
                crate::ops::Constant::Number(value) => {
                    constant = Some((
                        instruction.a,
                        crate::stencil_numeric_integer_selection::exact_i32(*value)?,
                    ));
                }
                crate::ops::Constant::Undefined => {}
                _ => return None,
            },
            Opcode::InitLocal => initialized = Some((instruction.a, instruction.b)),
            Opcode::Return => {}
            Opcode::Slow if marker(code, pc, None) => {}
            _ => return None,
        }
    }
    let (register, value) = constant?;
    let (slot, source) = initialized?;
    (register == source).then_some((slot, value))
}

fn select_test(code: CodeView<'_>, index_slot: u16) -> Option<i32> {
    let mut index_register = None;
    let mut bound = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadLocal | Opcode::LoadLocalChecked if instruction.b == index_slot => {
                index_register = Some(instruction.a)
            }
            Opcode::LoadConst => bound = Some((instruction.a, number(code, instruction.b)?)),
            Opcode::Binary => {
                let less = crate::ir::compact_binary_operator(instruction.flags)?
                    == crate::ops::BinaryOp::LessThan;
                (less && instruction.b == index_register? && instruction.c == bound?.0)
                    .then_some(())?;
            }
            Opcode::Return => {}
            _ => return None,
        }
    }
    Some(bound?.1)
}

fn select_update(code: CodeView<'_>, index_slot: u16) -> Option<()> {
    let mut update = false;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::UpdateLocal if instruction.flags == 0 && instruction.c == index_slot => {
                update = true
            }
            Opcode::Return => {}
            _ => return None,
        }
    }
    update.then_some(())
}

fn select_body(code: CodeView<'_>, index_slot: u16) -> Option<(u16, u16, Atom, u16, Atom, u8)> {
    let mut values = BTreeMap::new();
    let mut predicates = Vec::new();
    let mut condition_start = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        if predicates.len() == 2 {
            condition_start.get_or_insert(pc);
            break;
        }
        select_atom_instruction(
            code,
            pc,
            instruction,
            index_slot,
            &mut values,
            &mut predicates,
        )?;
    }
    let [(left_slot, left), (right_slot, right)] = predicates.as_slice() else {
        return None;
    };
    let (count_slot, truth_table) = truth_table(code, condition_start?, *left_slot, *right_slot)?;
    Some((
        count_slot,
        *left_slot,
        *left,
        *right_slot,
        *right,
        truth_table,
    ))
}

fn select_atom_instruction(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    index_slot: u16,
    values: &mut BTreeMap<u16, Symbol>,
    predicates: &mut Vec<(u16, Atom)>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadLocal | Opcode::LoadLocalChecked if instruction.b == index_slot => {
            values.insert(instruction.a, Symbol::Index);
        }
        Opcode::LoadConst => {
            values.insert(
                instruction.a,
                Symbol::Constant(number(code, instruction.b)?),
            );
        }
        Opcode::Binary => select_atom_binary(instruction, values)?,
        Opcode::InitLocal => {
            let Symbol::Predicate(atom) = values.get(&instruction.b)? else {
                return None;
            };
            predicates.push((instruction.a, *atom));
        }
        Opcode::Slow if atom_marker(code, pc, predicates) => {}
        _ => return None,
    }
    Some(())
}

fn atom_marker(code: CodeView<'_>, pc: usize, predicates: &[(u16, Atom)]) -> bool {
    match code.cold_at(pc) {
        Some(crate::ops::Op::MarkUninitialized { .. }) => true,
        Some(crate::ops::Op::MarkImmutable { slot }) => predicates
            .iter()
            .any(|(predicate_slot, _)| predicate_slot == slot),
        _ => false,
    }
}

fn select_atom_binary(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Symbol>,
) -> Option<()> {
    let operator = crate::ir::compact_binary_operator(instruction.flags)?;
    let left = values.get(&instruction.b)?.clone();
    let right = values.get(&instruction.c)?.clone();
    let result = match (operator, left, right) {
        (crate::ops::BinaryOp::BitwiseAnd, Symbol::Index, Symbol::Constant(value)) => {
            Symbol::Partial(AtomKind::MaskEquals, value)
        }
        (crate::ops::BinaryOp::Remainder, Symbol::Index, Symbol::Constant(value)) if value != 0 => {
            Symbol::Partial(AtomKind::RemainderEquals, value)
        }
        (
            crate::ops::BinaryOp::StrictEqual,
            Symbol::Partial(kind, operand),
            Symbol::Constant(expected),
        )
        | (
            crate::ops::BinaryOp::StrictEqual,
            Symbol::Constant(expected),
            Symbol::Partial(kind, operand),
        ) => Symbol::Predicate(Atom {
            kind,
            operand,
            expected,
        }),
        _ => return None,
    };
    values.insert(instruction.a, result);
    Some(())
}

fn truth_table(
    code: CodeView<'_>,
    start: usize,
    left_slot: u16,
    right_slot: u16,
) -> Option<(u16, u8)> {
    let mut count_slot = None;
    let mut table = 0_u8;
    for left in [false, true] {
        for right in [false, true] {
            let (slot, increments) =
                simulate_condition(code, start, left_slot, right_slot, left, right)?;
            if let (Some(existing), Some(slot)) = (count_slot, slot) {
                (existing == slot).then_some(())?;
            }
            count_slot = count_slot.or(slot);
            table |= u8::from(increments) << ((u8::from(left) << 1) | u8::from(right));
        }
    }
    Some((count_slot?, table))
}

fn simulate_condition(
    code: CodeView<'_>,
    start: usize,
    left_slot: u16,
    right_slot: u16,
    left: bool,
    right: bool,
) -> Option<(Option<u16>, bool)> {
    let mut values = BTreeMap::new();
    let mut pc = start;
    let mut incremented = None;
    for _ in 0..code.len().saturating_mul(2) {
        if pc >= code.len() {
            return Some((incremented, incremented.is_some()));
        }
        let instruction = code.instruction(pc)?;
        pc = simulate_instruction(
            code,
            instruction,
            pc,
            left_slot,
            right_slot,
            left,
            right,
            &mut values,
            &mut incremented,
        )?;
    }
    None
}

fn simulate_instruction(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    pc: usize,
    left_slot: u16,
    right_slot: u16,
    left: bool,
    right: bool,
    values: &mut BTreeMap<u16, FlowValue>,
    incremented: &mut Option<u16>,
) -> Option<usize> {
    let next = pc + 1;
    match instruction.opcode {
        Opcode::Slow if marker(code, pc, Some([left_slot, right_slot])) => {}
        Opcode::LoadLocal | Opcode::LoadLocalChecked => {
            simulate_load(instruction, left_slot, right_slot, left, right, values)
        }
        Opcode::LoadConst => simulate_constant(code, instruction, values)?,
        Opcode::Move => simulate_move(instruction, values)?,
        Opcode::Unary => simulate_unary(instruction, values)?,
        Opcode::Binary => simulate_increment(instruction, values)?,
        Opcode::StoreLocal => simulate_store(instruction, values, incremented)?,
        Opcode::JumpIfFalse => return simulate_branch(instruction, next, values),
        Opcode::Jump => return Some(usize::from(instruction.a)),
        _ => return None,
    }
    Some(next)
}

fn simulate_load(
    instruction: crate::ir::Instruction,
    left_slot: u16,
    right_slot: u16,
    left: bool,
    right: bool,
    values: &mut BTreeMap<u16, FlowValue>,
) {
    let value = match instruction.b {
        slot if slot == left_slot => FlowValue::Bool(left),
        slot if slot == right_slot => FlowValue::Bool(right),
        slot => FlowValue::Local(slot),
    };
    values.insert(instruction.a, value);
}

fn simulate_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, FlowValue>,
) -> Option<()> {
    let one = matches!(code.constant(instruction.b), Some(crate::ops::Constant::Number(value)) if *value == 1.0);
    values.insert(
        instruction.a,
        if one {
            FlowValue::One
        } else {
            FlowValue::Other
        },
    );
    Some(())
}

fn simulate_move(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, FlowValue>,
) -> Option<()> {
    values.insert(instruction.a, *values.get(&instruction.b)?);
    Some(())
}

fn simulate_unary(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, FlowValue>,
) -> Option<()> {
    let operator = crate::ir::compact_unary_operator(instruction.flags)?;
    let value = match (operator, values.get(&instruction.b)?) {
        (crate::ops::UnaryOp::Not, FlowValue::Bool(value)) => FlowValue::Bool(!value),
        (crate::ops::UnaryOp::ToNumeric, _) => FlowValue::Other,
        _ => return None,
    };
    values.insert(instruction.a, value);
    Some(())
}

fn simulate_increment(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, FlowValue>,
) -> Option<()> {
    (crate::ir::compact_binary_operator(instruction.flags)? == crate::ops::BinaryOp::NumericAdd)
        .then_some(())?;
    let slot = match (values.get(&instruction.b)?, values.get(&instruction.c)?) {
        (FlowValue::Local(slot), FlowValue::One) | (FlowValue::One, FlowValue::Local(slot)) => {
            *slot
        }
        _ => return None,
    };
    values.insert(instruction.a, FlowValue::Increment(slot));
    Some(())
}

fn simulate_store(
    instruction: crate::ir::Instruction,
    values: &BTreeMap<u16, FlowValue>,
    incremented: &mut Option<u16>,
) -> Option<()> {
    let FlowValue::Increment(slot) = values.get(&instruction.b)? else {
        return None;
    };
    (*slot == instruction.a).then_some(())?;
    *incremented = Some(*slot);
    Some(())
}

fn simulate_branch(
    instruction: crate::ir::Instruction,
    next: usize,
    values: &BTreeMap<u16, FlowValue>,
) -> Option<usize> {
    let FlowValue::Bool(value) = values.get(&instruction.a)? else {
        return None;
    };
    Some(if *value {
        next
    } else {
        usize::from(instruction.b)
    })
}

fn marker(code: CodeView<'_>, pc: usize, allowed: Option<[u16; 2]>) -> bool {
    match code.cold_at(pc) {
        Some(crate::ops::Op::MarkUninitialized { slot, .. }) => {
            allowed.is_none_or(|slots| slots.contains(slot))
        }
        Some(crate::ops::Op::MarkImmutable { slot }) => {
            allowed.is_some_and(|slots| slots.contains(slot))
        }
        _ => false,
    }
}

fn iteration_slots_match(slots: &[u16], index: u16, left: u16, right: u16) -> Option<()> {
    (slots.len() == 3
        && [index, left, right]
            .into_iter()
            .all(|slot| slots.contains(&slot)))
    .then_some(())
}

fn number(code: CodeView<'_>, constant: u16) -> Option<i32> {
    let crate::ops::Constant::Number(value) = code.constant(constant)? else {
        return None;
    };
    crate::stencil_numeric_integer_selection::exact_i32(*value)
}
