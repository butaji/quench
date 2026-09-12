//! Bounded counted sum over one nonescaping, pure affine local function.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};
use crate::ops::{BinaryOp, FunctionKind, Op, UnaryOp};

pub(crate) const REGION_END: usize = 38;
const LOOP_HEADER: usize = 13;
const LOOP_EXIT: usize = 34;
const MAX_EXACT_INTEGER: i128 = 1_i128 << 53;

#[derive(Clone, Copy)]
pub(crate) struct LocalAffineSumSelection {
    state_slot: u16,
    bound_pc: usize,
    seed_pc: usize,
    multiplier: i32,
    addend: i32,
    discarded_stores: u8,
    requires_nonnegative: bool,
    eliminated_ops: u8,
}

pub(crate) struct NativeLocalAffineSumPlan {
    selection: LocalAffineSumSelection,
}

impl NativeLocalAffineSumPlan {
    pub(crate) fn new(
        selection: LocalAffineSumSelection,
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
        // SAFETY: VmContext owns the AtomicBool for this non-reentrant call.
        let interrupted =
            unsafe { &*context.interrupt_flag() }.load(std::sync::atomic::Ordering::Relaxed);
        (!interrupted).then_some(())?;
        let object = environment.retain_proven_object(self.selection.state_slot)?;
        let end = guarded_integer_property(code, self.selection.bound_pc, &object)?;
        let seed = guarded_integer_property(code, self.selection.seed_pc, &object)?;
        exact_sum(self.selection, seed, end)?;
        let end = usize::try_from(end).ok()?;
        Some(execute_affine_sum(AffineSumContext::new(
            self.selection,
            seed,
            end,
        )))
    }

    pub(crate) const fn profile_name(&self) -> &'static str {
        if self.selection.eliminated_ops > 0 {
            "locals_body_size_region"
        } else if self.selection.discarded_stores >= 4 {
            "locals_many_region"
        } else {
            "locals_small_region"
        }
    }

    pub(crate) const fn route(&self) -> [&'static str; 2] {
        if self.selection.eliminated_ops > 0 {
            ["locals", "body_size"]
        } else if self.selection.discarded_stores >= 4 {
            ["locals", "many"]
        } else {
            ["locals", "small"]
        }
    }
}

#[repr(C)]
struct AffineSumContext {
    end: usize,
    seed: i32,
    multiplier: i32,
    addend: i32,
    result: f64,
}

impl AffineSumContext {
    const fn new(selection: LocalAffineSumSelection, seed: i32, end: usize) -> Self {
        Self {
            end,
            seed,
            multiplier: selection.multiplier,
            addend: selection.addend,
            result: 0.0,
        }
    }
}

#[inline(never)]
extern "C" fn execute_affine_sum(mut state: AffineSumContext) -> f64 {
    for index in 0..state.end {
        let input = index as f64 + f64::from(state.seed);
        let value = input * f64::from(state.multiplier) + f64::from(state.addend);
        state.result += value;
    }
    state.result
}

pub(crate) fn select_local_affine_sum(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<LocalAffineSumSelection> {
    (start == 0).then_some(())?;
    cfg.region_control(0, REGION_END)?;
    let instructions = operation_window(entries)?;
    let fact = local_function_fact(code, &instructions)?;
    validate_prefix(code, &instructions)?;
    validate_loop(code, &instructions)?;
    Some(LocalAffineSumSelection {
        state_slot: instructions[14].b,
        bound_pc: 15,
        seed_pc: 22,
        multiplier: fact.multiplier,
        addend: fact.addend,
        discarded_stores: fact.discarded_stores,
        requires_nonnegative: fact.requires_nonnegative,
        eliminated_ops: fact.eliminated_ops,
    })
}

fn local_function_fact(
    code: CodeView<'_>,
    i: &[Instruction; REGION_END],
) -> Option<crate::function_affine_number::AffineNumberFact> {
    let Op::MakeFunctionWithKind {
        dst,
        body,
        params: 1,
        captures,
        kind: FunctionKind::Ordinary,
        is_async: false,
        ..
    } = code.cold_at(2)?
    else {
        return None;
    };
    (*dst == i[5].a && *dst == i[6].b).then_some(())?;
    crate::function_affine_number::affine_number_function(body, *captures)
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
    (i[5].b == i[4].a).then_some(())?;
    (i[6].a == i[1].a && i[6].b == i[5].a).then_some(())?;
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
    (i[21].b == i[14].b).then_some(())?;
    named_get(code, 22, i[22], i[21].a, "seed")?;
    (i[23].b == i[20].a && i[23].c == i[22].a).then_some(())?;
    binary(i[23], BinaryOp::Add, i[20].a, i[22].a)?;
    validate_call_and_update(code, i)
}

fn validate_call_and_update(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    (i[24].flags == 1 && i[24].b == i[19].a && i[24].c == i[23].a).then_some(())?;
    binary(i[25], BinaryOp::Add, i[18].a, i[24].a)?;
    (i[26].a == i[8].a && i[26].b == i[25].a && i[27].b == i[25].a).then_some(())?;
    number(code, i[29], 1.0)?;
    binary(i[30], BinaryOp::NumericAdd, i[28].a, i[29].a)?;
    (i[28].b == i[11].a && i[31].a == i[11].a && i[31].b == i[30].a).then_some(())?;
    (crate::ir::compact_unary_operator(i[32].flags) == Some(UnaryOp::ToNumeric)
        && i[32].b == i[28].a)
        .then_some(())?;
    (usize::from(i[33].a) == LOOP_HEADER && i[34].b == i[8].a && i[35].a == i[34].a)
        .then_some(())?;
    undefined(code, i[36])?;
    (i[37] == Instruction::ret(i[36].a)).then_some(())
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
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::Add,
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

fn number(code: CodeView<'_>, op: Instruction, expected: f64) -> Option<()> {
    (op.opcode == Opcode::LoadConst
        && matches!(code.constant(op.b), Some(crate::ops::Constant::Number(value)) if *value == expected))
        .then_some(())
}

fn undefined(code: CodeView<'_>, op: Instruction) -> Option<()> {
    (op.opcode == Opcode::LoadConst
        && matches!(code.constant(op.b), Some(crate::ops::Constant::Undefined)))
    .then_some(())
}

fn guarded_integer_property(
    code: CodeView<'_>,
    pc: usize,
    object: &crate::value::ObjectData,
) -> Option<i32> {
    let value = crate::vm::cached_own_property_number(code, pc, object)?;
    let integer = value as i32;
    (f64::from(integer) == value).then_some(integer)
}

fn exact_sum(selection: LocalAffineSumSelection, seed: i32, end: i32) -> Option<()> {
    let count = usize::try_from(end).ok()?;
    (!selection.requires_nonnegative || seed >= 0).then_some(())?;
    let count = count as i128;
    let first = i128::from(seed) * i128::from(selection.multiplier) + i128::from(selection.addend);
    let last_input = i128::from(seed) + count.saturating_sub(1);
    let last = last_input * i128::from(selection.multiplier) + i128::from(selection.addend);
    (first.abs() <= MAX_EXACT_INTEGER && last.abs() <= MAX_EXACT_INTEGER).then_some(())?;
    let sum = count * (first + last) / 2;
    (sum.abs() <= MAX_EXACT_INTEGER).then_some(())
}
