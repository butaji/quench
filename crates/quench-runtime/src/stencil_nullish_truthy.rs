//! Tagged nullish/truthy decision region with a closed pre-entry domain.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};
use std::{cell::RefCell, rc::Rc};

const REGION_LEN: usize = 14;
const OPERATIONS: [Opcode; REGION_LEN] = [
    Opcode::LoadLocal, Opcode::Unary, Opcode::JumpIfFalse, Opcode::LoadLocal,
    Opcode::JumpIfFalse, Opcode::LoadConst, Opcode::Move, Opcode::Jump,
    Opcode::LoadConst, Opcode::Move, Opcode::Move, Opcode::Jump, Opcode::Move,
    Opcode::Return,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NullishTruthySelection {
    value_slot: u16,
    fallback_slot: u16,
}

pub(crate) struct NativeNullishTruthyPlan {
    selection: NullishTruthySelection,
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    view: crate::stencil_select::PhysicalStencilView,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<extern "C" fn(u64, u64) -> u64>>,
}

impl NativeNullishTruthyPlan {
    pub(crate) fn new(
        selection: NullishTruthySelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.local_fusions.predicate().then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::nullish_truthy_branch_return_region_key(),
            crate::stencil_select::RegionAbi::ScalarWordPair,
        )?;
        view.generated.then_some(Self {
            selection, owner, view,
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
        })
    }

    pub(crate) const fn span(&self) -> usize {
        REGION_LEN
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<u64> {
        let value = environment.proven_tagged_bits(self.selection.value_slot)?;
        let fallback = environment.proven_tagged_bits(self.selection.fallback_slot)?;
        input_domain(value, fallback)?;
        let entry = self.entry()?;
        let lease = crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry).ok()?;
        lease.invoke(|call| call(value, fallback)).ok()
    }

    fn entry(
        &mut self,
    ) -> Option<crate::stencil_arena::EntryToken<extern "C" fn(u64, u64) -> u64>> {
        if let Some(entry) = self.installed.filter(|entry| self.owner.borrow().entry_token_is_live(*entry)) {
            return Some(entry);
        }
        self.installed = None;
        let site = crate::quickening::QuickeningSite::<4>::new(Opcode::Unary);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let address = self.owner.borrow_mut().render_physical_view_or_get(
            &mut self.cache, self.view, &values,
        ).ok()?;
        self.owner.borrow_mut().make_executable(address).ok()?;
        let entry = self.owner.borrow().owned_word_pair_entry(address).ok()?;
        self.installed = Some(entry);
        Some(entry)
    }
}

fn input_domain(value: u64, fallback: u64) -> Option<()> {
    use crate::tagged_value::DecodedValue;
    let value = crate::tagged_value::TaggedValue::from_bits(value).decode();
    let fallback = crate::tagged_value::TaggedValue::from_bits(fallback).decode();
    matches!(value, DecodedValue::Number(_) | DecodedValue::Null | DecodedValue::Undefined)
        .then_some(())?;
    matches!(fallback, DecodedValue::Bool(_)).then_some(())
}

pub(crate) fn select_nullish_truthy(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<NullishTruthySelection> {
    let end = start.checked_add(REGION_LEN)?;
    let instructions = entries.get(start..end)?;
    (start == 0 && instructions.iter().zip(OPERATIONS).all(|(entry, opcode)| {
        entry.instruction.opcode == opcode
    })).then_some(())?;
    cfg.region_entry_is_legal(start, end).then_some(())?;
    bindings(code, instructions)
}

fn bindings(code: CodeView<'_>, entries: &[BaselineEntry]) -> Option<NullishTruthySelection> {
    let i = entries.iter().map(|entry| entry.instruction).collect::<Vec<_>>();
    unary_nullish(i[1], i[0].a)?;
    branch(i[2], i[1].a, 12)?;
    branch(i[4], i[3].a, 8)?;
    number(code, i[5], 41.0)?;
    move_value(i[6], i[5].a)?;
    jump(i[7], 10)?;
    number(code, i[8], 7.0)?;
    move_pair(i[9], i[6].a, i[8].a)?;
    move_value(i[10], i[6].a)?;
    jump(i[11], 13)?;
    move_pair(i[12], i[10].a, i[0].a)?;
    returns(i[13], i[10].a)?;
    Some(NullishTruthySelection { value_slot: i[0].b, fallback_slot: i[3].b })
}

fn unary_nullish(instruction: Instruction, source: u16) -> Option<()> {
    (instruction.flags == crate::ir::compact_unary_id(crate::ops::UnaryOp::IsNullish)
        && instruction.b == source).then_some(())
}

fn number(code: CodeView<'_>, instruction: Instruction, expected: f64) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else { return None };
    (value.to_bits() == expected.to_bits()).then_some(())
}

fn branch(instruction: Instruction, condition: u16, target: u16) -> Option<()> {
    (instruction.a == condition && instruction.b == target).then_some(())
}

fn jump(instruction: Instruction, target: u16) -> Option<()> {
    (instruction.a == target).then_some(())
}

fn move_value(instruction: Instruction, source: u16) -> Option<()> {
    (instruction.b == source).then_some(())
}

fn move_pair(instruction: Instruction, destination: u16, source: u16) -> Option<()> {
    (instruction.a == destination && instruction.b == source).then_some(())
}

fn returns(instruction: Instruction, source: u16) -> Option<()> {
    (instruction.a == source).then_some(())
}
