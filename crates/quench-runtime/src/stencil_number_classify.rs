//! Whole-region Number classification over canonical residual control flow.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};
use std::{cell::RefCell, rc::Rc};

const REGION_LEN: usize = 39;
const OPERATIONS: [Opcode; REGION_LEN] = [
    Opcode::LoadLocal,
    Opcode::LoadLocal,
    Opcode::Binary,
    Opcode::LoadConst,
    Opcode::JumpIfFalse,
    Opcode::LoadConst,
    Opcode::Return,
    Opcode::LoadConst,
    Opcode::Jump,
    Opcode::LoadLocal,
    Opcode::LoadConst,
    Opcode::Binary,
    Opcode::JumpIfFalse,
    Opcode::LoadConst,
    Opcode::LoadLocal,
    Opcode::Div,
    Opcode::LoadConst,
    Opcode::Unary,
    Opcode::Binary,
    Opcode::Move,
    Opcode::Jump,
    Opcode::Move,
    Opcode::LoadConst,
    Opcode::JumpIfFalse,
    Opcode::LoadConst,
    Opcode::Return,
    Opcode::LoadConst,
    Opcode::Jump,
    Opcode::LoadLocal,
    Opcode::LoadConst,
    Opcode::Binary,
    Opcode::LoadConst,
    Opcode::JumpIfFalse,
    Opcode::LoadConst,
    Opcode::Return,
    Opcode::LoadConst,
    Opcode::Jump,
    Opcode::LoadConst,
    Opcode::Return,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NumberClassifySelection {
    source_slot: u16,
}

pub(crate) struct NativeNumberClassifyPlan {
    selection: NumberClassifySelection,
    view: crate::stencil_select::PhysicalStencilView,
    physical: crate::stencil_installation::SharedPhysicalEntry<extern "C" fn(f64) -> f64>,
}

impl NativeNumberClassifyPlan {
    pub(crate) fn new(
        selection: NumberClassifySelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.local_fusions.numeric().then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::number_classify_branch_return_region_key(),
            crate::stencil_select::RegionAbi::ScalarF64Unary,
        )?;
        view.generated.then_some(Self {
            selection,
            view,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
        })
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<f64> {
        let value = environment.get_number(self.selection.source_slot)?;
        let entry = self.entry()?;
        self.physical.invoke(entry, |call| call(value)).ok()
    }

    pub(crate) const fn span(&self) -> usize {
        REGION_LEN
    }

    fn entry(&mut self) -> Option<crate::stencil_arena::EntryToken<extern "C" fn(f64) -> f64>> {
        let site = crate::quickening::QuickeningSite::<4>::new(Opcode::Binary);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        self.physical
            .entry(
                |owner, cache| {
                    let address = owner
                        .borrow_mut()
                        .render_physical_view_or_get(cache, self.view, &values)?;
                    owner.borrow_mut().make_executable(address)?;
                    Ok(address)
                },
                |pool, address| pool.owned_f64_unary_entry(address),
            )
            .ok()
    }
}

pub(crate) fn select_number_classify(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<NumberClassifySelection> {
    let end = start.checked_add(REGION_LEN)?;
    let instructions = entries.get(start..end)?;
    (start == 0
        && instructions
            .iter()
            .zip(OPERATIONS)
            .all(|(entry, opcode)| opcode.matches_physical_contract(entry.instruction.opcode)))
    .then_some(())?;
    cfg.region_entry_is_legal(start, end).then_some(())?;
    let source_slot = first_guard(code, instructions)?;
    negative_zero_guard(code, instructions, source_slot)?;
    negative_guard(code, instructions, source_slot)?;
    return_number(
        code,
        instructions[37].instruction,
        instructions[38].instruction,
        4.0,
    )?;
    Some(NumberClassifySelection { source_slot })
}

fn first_guard(code: CodeView<'_>, entries: &[BaselineEntry]) -> Option<u16> {
    let i = entries
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>();
    same_local(i[0], i[1])?;
    binary(i[2], crate::ops::BinaryOp::StrictNotEqual, i[0].a, i[1].a)?;
    branch(i[4], i[2].a, 9)?;
    return_number(code, i[5], i[6], 1.0)?;
    jump(i[8], 9)?;
    Some(i[0].b)
}

fn negative_zero_guard(code: CodeView<'_>, entries: &[BaselineEntry], slot: u16) -> Option<()> {
    let i = entries
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>();
    local(i[9], slot)?;
    number(code, i[10], 0.0)?;
    binary(i[11], crate::ops::BinaryOp::StrictEqual, i[9].a, i[10].a)?;
    branch(i[12], i[11].a, 21)?;
    number(code, i[13], 1.0)?;
    local(i[14], slot)?;
    arithmetic(i[15], Opcode::Div, i[13].a, i[14].a)?;
    number(code, i[16], f64::INFINITY)?;
    unary_minus(i[17], i[16].a)?;
    binary(i[18], crate::ops::BinaryOp::StrictEqual, i[15].a, i[17].a)?;
    move_to(i[19], i[18].a)?;
    jump(i[20], 22)?;
    move_pair(i[21], i[19].a, i[11].a)?;
    branch(i[23], i[19].a, 28)?;
    return_number(code, i[24], i[25], 2.0)?;
    jump(i[27], 28)
}

fn negative_guard(code: CodeView<'_>, entries: &[BaselineEntry], slot: u16) -> Option<()> {
    let i = entries
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>();
    local(i[28], slot)?;
    number(code, i[29], 0.0)?;
    binary(i[30], crate::ops::BinaryOp::LessThan, i[28].a, i[29].a)?;
    branch(i[32], i[30].a, 37)?;
    return_number(code, i[33], i[34], 3.0)?;
    jump(i[36], 37)
}

fn same_local(left: Instruction, right: Instruction) -> Option<()> {
    local(left, left.b)?;
    local(right, left.b)
}

fn local(instruction: Instruction, slot: u16) -> Option<()> {
    (instruction.opcode == Opcode::LoadLocal && instruction.b == slot).then_some(())
}

fn number(code: CodeView<'_>, instruction: Instruction, expected: f64) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    (instruction.opcode == Opcode::LoadConst && value.to_bits() == expected.to_bits()).then_some(())
}

fn return_number(
    code: CodeView<'_>,
    load: Instruction,
    ret: Instruction,
    expected: f64,
) -> Option<()> {
    number(code, load, expected)?;
    return_number_register(ret, load.a)
}

fn binary(
    instruction: Instruction,
    operator: crate::ops::BinaryOp,
    lhs: u16,
    rhs: u16,
) -> Option<()> {
    (instruction.opcode.binary_operator(instruction.flags) == Some(operator)
        && instruction.b == lhs
        && instruction.c == rhs)
        .then_some(())
}

fn arithmetic(instruction: Instruction, opcode: Opcode, lhs: u16, rhs: u16) -> Option<()> {
    (instruction.opcode == opcode && instruction.b == lhs && instruction.c == rhs).then_some(())
}

fn unary_minus(instruction: Instruction, source: u16) -> Option<()> {
    (instruction.opcode == Opcode::Unary
        && instruction.flags == crate::ir::compact_unary_id(crate::ops::UnaryOp::Minus)
        && instruction.b == source)
        .then_some(())
}

fn branch(instruction: Instruction, condition: u16, target: u16) -> Option<()> {
    (instruction.opcode == Opcode::JumpIfFalse
        && instruction.a == condition
        && instruction.b == target)
        .then_some(())
}

fn jump(instruction: Instruction, target: u16) -> Option<()> {
    (instruction.opcode == Opcode::Jump && instruction.a == target).then_some(())
}

fn return_number_register(instruction: Instruction, source: u16) -> Option<()> {
    (instruction.opcode == Opcode::Return && instruction.a == source).then_some(())
}

fn move_to(instruction: Instruction, source: u16) -> Option<()> {
    (instruction.opcode == Opcode::Move && instruction.b == source).then_some(())
}

fn move_pair(instruction: Instruction, destination: u16, source: u16) -> Option<()> {
    (instruction.opcode == Opcode::Move && instruction.a == destination && instruction.b == source)
        .then_some(())
}
