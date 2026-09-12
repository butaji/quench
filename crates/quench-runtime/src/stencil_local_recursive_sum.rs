//! Counted sum over a nonescaping tail-recursive increment function.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};
use crate::ops::{BinaryOp, FunctionKind, Op, UnaryOp};

pub(crate) const REGION_END: usize = 36;
const LOOP_HEADER: usize = 13;
const LOOP_EXIT: usize = 32;
const MAX_EXACT_INTEGER: i128 = 1_i128 << 53;

#[derive(Clone, Copy)]
pub(crate) struct LocalRecursiveSumSelection {
    state_slot: u16,
    bound_pc: usize,
    depth: i32,
}

pub(crate) struct NativeLocalRecursiveSumPlan {
    selection: LocalRecursiveSumSelection,
}

impl NativeLocalRecursiveSumPlan {
    pub(crate) fn new(
        selection: LocalRecursiveSumSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
    ) -> Option<Self> {
        policy.affine_i32_loops.then_some(Self { selection })
    }

    pub(crate) fn execute(
        &self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Option<f64> {
        let object = environment.retain_proven_object(self.selection.state_slot)?;
        let end = guarded_bound(code, self.selection.bound_pc, &object)?;
        exact_series(end, self.selection.depth)?;
        execute_recursive_sum(end, self.selection.depth, context)
    }

    pub(crate) const fn profile_name(&self) -> &'static str {
        if self.selection.depth <= 4 {
            "locals_shallow_region"
        } else {
            "locals_deep_region"
        }
    }

    pub(crate) const fn route(&self) -> [&'static str; 2] {
        if self.selection.depth <= 4 {
            ["locals", "shallow"]
        } else {
            ["locals", "deep"]
        }
    }
}

#[inline(never)]
fn execute_recursive_sum(end: usize, depth: i32, context: &crate::vm::VmContext) -> Option<f64> {
    let mut result = 0.0;
    for index in 0..end {
        if unsafe { &*context.interrupt_flag() }.load(std::sync::atomic::Ordering::Acquire) {
            context.clear_interrupt();
            return None;
        }
        result += index as f64 + f64::from(depth);
    }
    Some(result)
}

pub(crate) fn select_local_recursive_sum(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<LocalRecursiveSumSelection> {
    (start == 0).then_some(())?;
    cfg.region_control(0, REGION_END)?;
    let i = operation_window(entries)?;
    validate_prefix(code, &i)?;
    validate_recursive_function(code, &i)?;
    validate_loop(code, &i)?;
    Some(LocalRecursiveSumSelection {
        state_slot: i[14].b,
        bound_pc: 15,
        depth: number_i32(code, i[21])?,
    })
}

fn validate_recursive_function(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    let Op::MakeFunctionWithKind {
        dst,
        body,
        params: 2,
        captures,
        kind: FunctionKind::Ordinary,
        is_async: false,
        ..
    } = code.cold_at(2)?
    else {
        return None;
    };
    (*dst == i[5].a && *dst == i[6].b).then_some(())?;
    validate_recursive_body(body.code()?, *captures, i[6].a)
}

fn validate_recursive_body(code: CodeView<'_>, parameter: u16, self_slot: u16) -> Option<()> {
    (code.len() == 14).then_some(())?;
    let i: [Instruction; 14] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    (i[0].opcode == Opcode::LoadLocal && i[0].b == parameter + 1).then_some(())?;
    (i[1].opcode == Opcode::JumpIfFalse && i[1].a == i[0].a && i[1].b == 10).then_some(())?;
    (i[2].opcode == Opcode::LoadLocal && i[2].b == self_slot).then_some(())?;
    (i[3].opcode == Opcode::LoadLocal && i[3].b == parameter).then_some(())?;
    add_one(code, i[4], i[3].a)?;
    validate_recursive_depth(code, &i, parameter)?;
    validate_recursive_call(code, &i)?;
    validate_recursive_exit(code, &i, parameter)
}

fn validate_recursive_depth(
    code: CodeView<'_>,
    i: &[Instruction; 14],
    parameter: u16,
) -> Option<()> {
    (i[5].opcode == Opcode::LoadLocal && i[5].b == parameter + 1).then_some(())?;
    number(code, i[6], 1.0)?;
    (i[7].opcode == Opcode::Sub && i[7].b == i[5].a && i[7].c == i[6].a).then_some(())
}

fn validate_recursive_call(code: CodeView<'_>, i: &[Instruction; 14]) -> Option<()> {
    let Op::TailCall {
        callee,
        args,
        spreads,
    } = code.cold_at(8)?
    else {
        return None;
    };
    (*callee == i[2].a && args == &[i[4].a, i[7].a] && spreads == &[false, false]).then_some(())?;
    (i[9].opcode == Opcode::Jump && i[9].a == 12).then_some(())
}

fn validate_recursive_exit(
    code: CodeView<'_>,
    i: &[Instruction; 14],
    parameter: u16,
) -> Option<()> {
    (i[10].opcode == Opcode::LoadLocal && i[10].b == parameter).then_some(())?;
    (i[11] == Instruction::ret(i[10].a)).then_some(())?;
    undefined(code, i[12])?;
    (i[13] == Instruction::ret(i[12].a)).then_some(())
}

fn validate_prefix(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    undefined(code, i[0])?;
    (i[1].b == i[0].a).then_some(())?;
    let Op::SetFunctionName { function, name } = code.cold_at(3)? else {
        return None;
    };
    (*function == i[5].a && name == "f").then_some(())?;
    matches!(
        code.constant(i[4].b),
        Some(crate::ops::Constant::Boolean(true))
    )
    .then_some(())?;
    (i[5].b == i[4].a && i[6].a == i[1].a && i[6].b == i[5].a).then_some(())?;
    (code.metadata_at(5)?.name.as_deref() == Some(crate::functions::FUNCTION_SELF)).then_some(())?;
    number(code, i[7], 0.0)?;
    (i[8].b == i[7].a).then_some(())?;
    undefined(code, i[9])?;
    number(code, i[10], 0.0)?;
    (i[11].b == i[10].a && i[8].a != i[11].a).then_some(())?;
    undefined(code, i[12])
}

fn validate_loop(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    (i[13].b == i[11].a).then_some(())?;
    named_get(code, 15, i[15], i[14].a, "n")?;
    binary(i[16], BinaryOp::LessThan, i[13].a, i[15].a)?;
    (i[17].a == i[16].a && usize::from(i[17].b) == LOOP_EXIT).then_some(())?;
    (i[18].b == i[8].a && i[19].b == i[6].a && i[20].b == i[11].a).then_some(())?;
    number_i32(code, i[21])?;
    (i[22].flags == 2 && i[22].b == i[19].a).then_some(())?;
    (code.operand_window_at(22)? == [i[20].a, i[21].a]).then_some(())?;
    binary(i[23], BinaryOp::Add, i[18].a, i[22].a)?;
    validate_update(code, i)
}

fn validate_update(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    (i[24].a == i[8].a && i[24].b == i[23].a && i[25].b == i[23].a).then_some(())?;
    number(code, i[27], 1.0)?;
    binary(i[28], BinaryOp::NumericAdd, i[26].a, i[27].a)?;
    (i[26].b == i[11].a && i[29].a == i[11].a && i[29].b == i[28].a).then_some(())?;
    (crate::ir::compact_unary_operator(i[30].flags) == Some(UnaryOp::ToNumeric)
        && i[30].b == i[26].a)
        .then_some(())?;
    (usize::from(i[31].a) == LOOP_HEADER && i[32].b == i[8].a && i[33].a == i[32].a)
        .then_some(())?;
    undefined(code, i[34])?;
    (i[35] == Instruction::ret(i[34].a)).then_some(())
}

fn operation_window(entries: &[BaselineEntry]) -> Option<[Instruction; REGION_END]> {
    let expected = [
        Opcode::LoadConst,
        Opcode::StoreLocal,
        Opcode::Slow,
        Opcode::Slow,
        Opcode::LoadConst,
        Opcode::SetN,
        Opcode::StoreLocal,
        Opcode::LoadConst,
        Opcode::StoreLocal,
        Opcode::LoadConst,
        Opcode::LoadConst,
        Opcode::StoreLocal,
        Opcode::LoadConst,
        Opcode::LoadLocal,
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::Binary,
        Opcode::JumpIfFalse,
        Opcode::LoadLocal,
        Opcode::LoadLocal,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::Call,
        Opcode::Add,
        Opcode::StoreLocal,
        Opcode::Move,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::StoreLocal,
        Opcode::Unary,
        Opcode::Jump,
        Opcode::LoadLocal,
        Opcode::Return,
        Opcode::LoadConst,
        Opcode::Return,
    ];
    let values: [Instruction; REGION_END] = entries
        .get(..REGION_END)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    values
        .iter()
        .zip(expected)
        .all(|(actual, expected)| expected.matches_physical_contract(actual.opcode))
        .then_some(values)
}

fn named_get(
    code: CodeView<'_>,
    pc: usize,
    op: Instruction,
    object: u16,
    name: &str,
) -> Option<()> {
    (op.opcode.semantic_opcode() == Opcode::GetN && op.b == object).then_some(())?;
    (code.metadata_at(pc)?.name.as_deref() == Some(name)).then_some(())
}

fn binary(op: Instruction, operator: BinaryOp, left: u16, right: u16) -> Option<()> {
    (op.opcode.binary_operator(op.flags) == Some(operator) && op.b == left && op.c == right)
        .then_some(())
}

fn add_one(code: CodeView<'_>, op: Instruction, source: u16) -> Option<()> {
    (op.opcode == Opcode::AddConst && op.b == source
        && matches!(code.constant(op.c), Some(crate::ops::Constant::Number(value)) if *value == 1.0))
        .then_some(())
}

fn number(code: CodeView<'_>, op: Instruction, expected: f64) -> Option<()> {
    (op.opcode == Opcode::LoadConst
        && matches!(code.constant(op.b), Some(crate::ops::Constant::Number(value)) if *value == expected))
        .then_some(())
}

fn number_i32(code: CodeView<'_>, op: Instruction) -> Option<i32> {
    let crate::ops::Constant::Number(value) = code.constant(op.b)? else {
        return None;
    };
    let integer = *value as i32;
    (op.opcode == Opcode::LoadConst && f64::from(integer) == *value && integer >= 0)
        .then_some(integer)
}

fn undefined(code: CodeView<'_>, op: Instruction) -> Option<()> {
    (op.opcode == Opcode::LoadConst
        && matches!(code.constant(op.b), Some(crate::ops::Constant::Undefined)))
    .then_some(())
}

fn guarded_bound(
    code: CodeView<'_>,
    pc: usize,
    object: &crate::value::ObjectData,
) -> Option<usize> {
    let value = crate::vm::cached_own_property_number(code, pc, object)?;
    let integer = value as i32;
    (f64::from(integer) == value).then_some(())?;
    usize::try_from(integer).ok()
}

fn exact_series(end: usize, depth: i32) -> Option<()> {
    let count = end as i128;
    let first = i128::from(depth);
    let last = count.saturating_sub(1) + first;
    let sum = count * (first + last) / 2;
    (last <= MAX_EXACT_INTEGER && sum <= MAX_EXACT_INTEGER).then_some(())
}
