//! Two independent guarded int32 recurrences sharing one native backedge.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView, NativeDispatchError};
use std::{cell::RefCell, rc::Rc};

pub(crate) const REGION_END: usize = 46;
const SEED_OFFSET: usize = 1;
const BOUND_OFFSET: usize = 13;
const LOOP_HEADER_OFFSET: usize = 11;
const LOOP_BACKEDGE_OFFSET: usize = 39;
const LOOP_EXIT_OFFSET: usize = 40;
const ARRAY_RESULT_OFFSET: usize = 42;

#[derive(Clone, Copy)]
pub(crate) struct IndependentLoopSelection {
    start: usize,
    seed_pc: usize,
    bound_pc: usize,
    loop_header_pc: usize,
    loop_backedge_pc: usize,
    region_end_pc: usize,
    state_slot: u16,
    left_slot: u16,
    right_slot: u16,
    index_slot: u16,
    multiplier: i32,
}

pub(crate) enum IndependentLoopOutcome {
    Completed(crate::value::Value),
    Resume { pc: usize },
}

#[repr(C)]
struct IndependentLoopContext {
    index: usize,
    end: usize,
    left: i32,
    right: i32,
    multiplier: i32,
    _padding: u32,
    interrupt: *const std::sync::atomic::AtomicBool,
}

pub(crate) struct NativeIndependentLoopPlan {
    selection: IndependentLoopSelection,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<crate::stencil_arena::DispatchEntry>,
}

impl NativeIndependentLoopPlan {
    pub(crate) fn new(
        selection: IndependentLoopSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.numeric_i32_pair_loops.then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::numeric_independent_loop_region_key(),
            crate::stencil_select::RegionAbi::NumericI32PairLoop,
        )?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        let site = crate::quickening::QuickeningSite::<4>::new(Opcode::Binary);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let image = crate::stencil_region_layout::finalize_selected_leaf(view, &values).ok()?;
        Some(Self {
            selection,
            image,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
        })
    }

    pub(crate) fn execute(
        &mut self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
        vm: &crate::vm::VmContext,
    ) -> Result<Option<IndependentLoopOutcome>, NativeDispatchError> {
        let Some((seed, end)) = self.inputs(code, environment) else {
            return Ok(None);
        };
        let mut context = self.context(seed, end, vm);
        let status = self.invoke(&mut context)?;
        self.finish(status, context, environment, vm).map(Some)
    }

    fn inputs(
        &self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
    ) -> Option<(i32, usize)> {
        environment
            .with_proven_object(self.selection.state_slot, |object| {
                let seed =
                    crate::vm::cached_own_property_number(code, self.selection.seed_pc, object)?;
                let end =
                    crate::vm::cached_own_property_number(code, self.selection.bound_pc, object)?;
                let seed = exact_i32(seed)?;
                seed.checked_add(1)?;
                Some((seed, exact_bound(end)?))
            })
            .flatten()
    }

    fn context(&self, seed: i32, end: usize, vm: &crate::vm::VmContext) -> IndependentLoopContext {
        IndependentLoopContext {
            index: 0,
            end,
            left: seed,
            right: seed + 1,
            multiplier: self.selection.multiplier,
            _padding: 0,
            interrupt: vm.interrupt_flag(),
        }
    }

    fn invoke(&mut self, context: &mut IndependentLoopContext) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        self.physical
            .invoke(entry, |call| {
                call((context as *mut IndependentLoopContext).cast())
            })
            .map_err(|error| NativeDispatchError::Physical(format!("pair loop invoke: {error:?}")))
    }

    fn finish(
        &self,
        status: u64,
        context: IndependentLoopContext,
        environment: &crate::environment::Environment,
        vm: &crate::vm::VmContext,
    ) -> Result<IndependentLoopOutcome, NativeDispatchError> {
        if status == crate::vm::NATIVE_DISPATCH_INTERRUPT && context.index < context.end {
            vm.clear_interrupt();
            self.commit(&context, environment);
            return Ok(IndependentLoopOutcome::Resume {
                pc: self.selection.loop_header_pc,
            });
        }
        if status != crate::vm::NATIVE_DISPATCH_OK || context.index != context.end {
            return Err(NativeDispatchError::committed(
                self.selection.loop_backedge_pc,
                "independent recurrence returned incomplete progress",
            ));
        }
        let values = vec![
            crate::value::Value::Number(f64::from(context.left)),
            crate::value::Value::Number(f64::from(context.right)),
        ];
        Ok(IndependentLoopOutcome::Completed(
            crate::value::Value::array(values),
        ))
    }

    fn commit(
        &self,
        context: &IndependentLoopContext,
        environment: &crate::environment::Environment,
    ) {
        environment.set(
            self.selection.index_slot,
            crate::value::Value::Number(context.index as f64),
        );
        environment.set(
            self.selection.left_slot,
            crate::value::Value::Number(f64::from(context.left)),
        );
        environment.set(
            self.selection.right_slot,
            crate::value::Value::Number(f64::from(context.right)),
        );
    }

    fn entry(
        &mut self,
    ) -> Result<
        crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>,
        NativeDispatchError,
    > {
        let image = &self.image;
        self.physical
            .entry(
                |owner, cache| owner.borrow_mut().publish_region_image_or_get(cache, image),
                |pool, address| pool.owned_numeric_i32_pair_loop_entry(address),
            )
            .map_err(|error| NativeDispatchError::Physical(format!("pair loop entry: {error:?}")))
    }

    pub(crate) const fn start_pc(&self) -> usize {
        self.selection.start
    }

    pub(crate) const fn region_end_pc(&self) -> usize {
        self.selection.region_end_pc
    }
}

pub(crate) fn select_independent_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<IndependentLoopSelection> {
    let end = start.checked_add(REGION_END)?;
    cfg.region_control(start, end)?;
    let instructions = operation_window(entries, start)?;
    constants_and_operators(code, &instructions)?;
    bindings_match(code, &instructions, start)?;
    array_result_matches(code, &instructions, start)?;
    Some(IndependentLoopSelection {
        start,
        seed_pc: start.checked_add(SEED_OFFSET)?,
        bound_pc: start.checked_add(BOUND_OFFSET)?,
        loop_header_pc: start.checked_add(LOOP_HEADER_OFFSET)?,
        loop_backedge_pc: start.checked_add(LOOP_BACKEDGE_OFFSET)?,
        region_end_pc: end,
        state_slot: instructions[0].b,
        left_slot: instructions[2].a,
        right_slot: instructions[6].a,
        index_slot: instructions[9].a,
        multiplier: number_i32(code, instructions[17])?,
    })
}

fn operation_window(entries: &[BaselineEntry], start: usize) -> Option<[Instruction; REGION_END]> {
    let instructions: [Instruction; REGION_END] = entries
        .get(start..start.checked_add(REGION_END)?)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let expected = [
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::StoreLocal,
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::AddConst,
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
        Opcode::LoadConst,
        Opcode::Mul,
        Opcode::LoadLocal,
        Opcode::Add,
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::StoreLocal,
        Opcode::Move,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::Mul,
        Opcode::LoadLocal,
        Opcode::Add,
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::StoreLocal,
        Opcode::Move,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::StoreLocal,
        Opcode::Unary,
        Opcode::Jump,
        Opcode::LoadLocal,
        Opcode::LoadLocal,
        Opcode::Slow,
        Opcode::Return,
        Opcode::LoadConst,
        Opcode::Return,
    ];
    instructions
        .iter()
        .zip(expected)
        .all(|(actual, expected)| expected.matches_physical_contract(actual.opcode))
        .then_some(instructions)
}

fn constants_and_operators(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    (i[5].flags == 0).then_some(())?;
    exact_constant_id(code, i[5].c, 1.0)?;
    undefined_constant(code, i[7])?;
    exact_number(code, i[8], 0.0)?;
    undefined_constant(code, i[10])?;
    let multiplier = number_i32(code, i[17])?;
    (number_i32(code, i[26])? == multiplier).then_some(())?;
    exact_number(code, i[21], 0.0)?;
    exact_number(code, i[30], 0.0)?;
    exact_number(code, i[35], 1.0)?;
    undefined_constant(code, i[44])?;
    for (at, operator) in [
        (14, crate::ops::BinaryOp::LessThan),
        (22, crate::ops::BinaryOp::BitwiseOr),
        (31, crate::ops::BinaryOp::BitwiseOr),
        (36, crate::ops::BinaryOp::NumericAdd),
    ] {
        binary_operator(i[at], operator)?;
    }
    (crate::ir::compact_unary_operator(i[38].flags) == Some(crate::ops::UnaryOp::ToNumeric))
        .then_some(())
}

fn bindings_match(code: CodeView<'_>, i: &[Instruction; REGION_END], start: usize) -> Option<()> {
    let state = i[0].b;
    let left = i[2].a;
    let right = i[6].a;
    let index = i[9].a;
    ([left, right, index].into_iter().all(|slot| slot != state)
        && left != right
        && left != index
        && right != index)
        .then_some(())?;
    (i[1].b == i[0].a && i[2].b == i[1].a && i[4].b == i[3].a && i[3].b == state).then_some(())?;
    (i[5].b == i[4].a && i[6].b == i[5].a && i[9].b == i[8].a).then_some(())?;
    (i[11].b == index && i[12].b == state && i[13].b == i[12].a).then_some(())?;
    (i[14].b == i[11].a && i[14].c == i[13].a && i[15].a == i[14].a).then_some(())?;
    (usize::from(i[15].b) == start.checked_add(LOOP_EXIT_OFFSET)?
        && usize::from(i[39].a) == start.checked_add(LOOP_HEADER_OFFSET)?
        && usize::from(i[39].a) < start.checked_add(LOOP_BACKEDGE_OFFSET)?
        && usize::from(i[15].b) < start.checked_add(REGION_END)?)
    .then_some(())?;
    recurrence_bindings(i, 16, left, index)?;
    recurrence_bindings(i, 25, right, index)?;
    (i[34].b == index && i[36].b == i[34].a && i[36].c == i[35].a).then_some(())?;
    (i[37].a == index && i[37].b == i[36].a && i[38].b == i[34].a).then_some(())?;
    (i[40].b == left && i[41].b == right && i[45].a == i[44].a).then_some(())?;
    code.metadata_at(start.checked_add(SEED_OFFSET)?)?
        .name
        .as_deref()?;
    code.metadata_at(start.checked_add(4)?)?.name.as_deref()?;
    code.metadata_at(start.checked_add(BOUND_OFFSET)?)?
        .name
        .as_deref()?;
    Some(())
}

fn recurrence_bindings(
    i: &[Instruction; REGION_END],
    at: usize,
    slot: u16,
    index: u16,
) -> Option<()> {
    (i[at].b == slot && i[at + 2].b == i[at].a && i[at + 2].c == i[at + 1].a).then_some(())?;
    (i[at + 3].b == index).then_some(())?;
    (i[at + 4].b == i[at + 2].a && i[at + 4].c == i[at + 3].a).then_some(())?;
    (i[at + 6].b == i[at + 4].a && i[at + 6].c == i[at + 5].a).then_some(())?;
    (i[at + 7].a == slot && i[at + 7].b == i[at + 6].a && i[at + 8].b == i[at + 6].a).then_some(())
}

fn array_result_matches(
    code: CodeView<'_>,
    i: &[Instruction; REGION_END],
    start: usize,
) -> Option<()> {
    let crate::ops::Op::MakeArray { dst, elements } =
        code.cold_at(start.checked_add(ARRAY_RESULT_OFFSET)?)?
    else {
        return None;
    };
    (elements.as_slice() == [i[40].a, i[41].a] && *dst == i[43].a).then_some(())
}

fn binary_operator(instruction: Instruction, expected: crate::ops::BinaryOp) -> Option<()> {
    (instruction.opcode.binary_operator(instruction.flags) == Some(expected)).then_some(())
}
fn undefined_constant(code: CodeView<'_>, instruction: Instruction) -> Option<()> {
    matches!(
        code.constant(instruction.b),
        Some(crate::ops::Constant::Undefined)
    )
    .then_some(())
}
fn exact_number(code: CodeView<'_>, instruction: Instruction, expected: f64) -> Option<()> {
    (number_constant(code, instruction)?.to_bits() == expected.to_bits()).then_some(())
}
fn exact_constant_id(code: CodeView<'_>, constant: u16, expected: f64) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(constant)? else {
        return None;
    };
    (value.to_bits() == expected.to_bits()).then_some(())
}
fn number_constant(code: CodeView<'_>, instruction: Instruction) -> Option<f64> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    Some(*value)
}
fn number_i32(code: CodeView<'_>, instruction: Instruction) -> Option<i32> {
    exact_i32(number_constant(code, instruction)?)
}
fn exact_i32(value: f64) -> Option<i32> {
    if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
        return None;
    }
    let result = value as i32;
    (f64::from(result).to_bits() == value.to_bits()).then_some(result)
}
fn exact_bound(value: f64) -> Option<usize> {
    (value.is_finite() && value >= 0.0 && value < usize::MAX as f64 && value.fract() == 0.0)
        .then_some(())?;
    let integer = value as usize;
    (integer as f64 == value).then_some(integer)
}
