//! Guarded floating recurrence with a periodic numeric branch.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView, NativeDispatchError};
use std::{cell::RefCell, rc::Rc};

pub(crate) const REGION_END: usize = 39;
const LOOP_HEADER: usize = 7;
const LOOP_BACKEDGE: usize = 34;
const LOOP_EXIT: usize = 35;
const MAX_ITERATIONS: usize = 1 << 20;

#[derive(Clone, Copy)]
pub(crate) struct MixedLoopSelection {
    state_slot: u16,
    value_slot: u16,
    index_slot: u16,
    exceptional_increment: f64,
    ordinary_increment: f64,
    period: usize,
    value_modulus: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MixedLoopOutcome {
    Completed(f64),
    Resume { pc: usize },
}

#[repr(C)]
struct MixedLoopContext {
    index: usize,
    end: usize,
    value: f64,
    exceptional_increment: f64,
    ordinary_increment: f64,
    period: usize,
    interrupt: *const std::sync::atomic::AtomicBool,
}

pub(crate) struct NativeMixedLoopPlan {
    selection: MixedLoopSelection,
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

impl NativeMixedLoopPlan {
    pub(crate) fn new(
        selection: MixedLoopSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.numeric_f64_mixed_loops.then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::numeric_mixed_loop_region_key(),
            crate::stencil_select::RegionAbi::NumericF64MixedLoop,
        )?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        let site = crate::quickening::QuickeningSite::<4>::new(Opcode::Binary);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let image = crate::stencil_region_layout::finalize_selected_leaf(view, &values).ok()?;
        Some(Self {
            selection,
            owner,
            image,
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
        })
    }

    pub(crate) fn execute(
        &mut self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
        vm: &crate::vm::VmContext,
    ) -> Result<Option<MixedLoopOutcome>, NativeDispatchError> {
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
    ) -> Option<(f64, usize)> {
        environment
            .with_proven_object(self.selection.state_slot, |object| {
                let seed = crate::vm::cached_own_property_number(code, 1, object)?;
                let end = exact_bound(crate::vm::cached_own_property_number(code, 9, object)?)?;
                range_proves_remainder_identity(seed, end, self.selection)?;
                Some((seed, end))
            })
            .flatten()
    }

    fn context(&self, value: f64, end: usize, vm: &crate::vm::VmContext) -> MixedLoopContext {
        MixedLoopContext {
            index: 0,
            end,
            value,
            exceptional_increment: self.selection.exceptional_increment,
            ordinary_increment: self.selection.ordinary_increment,
            period: self.selection.period,
            interrupt: vm.interrupt_flag(),
        }
    }

    fn invoke(&mut self, context: &mut MixedLoopContext) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        let lease = crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry)
            .map_err(|error| {
                NativeDispatchError::Physical(format!("mixed loop lease: {error:?}"))
            })?;
        lease
            .invoke(|call| call((context as *mut MixedLoopContext).cast()))
            .map_err(|error| NativeDispatchError::Physical(format!("mixed loop invoke: {error:?}")))
    }

    fn finish(
        &self,
        status: u64,
        context: MixedLoopContext,
        environment: &crate::environment::Environment,
        vm: &crate::vm::VmContext,
    ) -> Result<MixedLoopOutcome, NativeDispatchError> {
        if status == crate::vm::NATIVE_DISPATCH_INTERRUPT && context.index < context.end {
            vm.clear_interrupt();
            self.commit(&context, environment);
            return Ok(MixedLoopOutcome::Resume { pc: LOOP_HEADER });
        }
        if status != crate::vm::NATIVE_DISPATCH_OK || context.index != context.end {
            return Err(NativeDispatchError::committed(
                LOOP_BACKEDGE,
                "mixed recurrence returned incomplete progress",
            ));
        }
        Ok(MixedLoopOutcome::Completed(context.value))
    }

    fn commit(&self, context: &MixedLoopContext, environment: &crate::environment::Environment) {
        environment.set(
            self.selection.index_slot,
            crate::value::Value::Number(context.index as f64),
        );
        environment.set(
            self.selection.value_slot,
            crate::value::Value::Number(context.value),
        );
    }

    fn entry(
        &mut self,
    ) -> Result<
        crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>,
        NativeDispatchError,
    > {
        if let Some(entry) = self
            .installed
            .filter(|entry| self.owner.borrow().entry_token_is_live(*entry))
        {
            return Ok(entry);
        }
        self.installed = None;
        let address = self
            .owner
            .borrow_mut()
            .publish_region_image_or_get(&mut self.cache, &self.image)
            .map_err(|error| {
                NativeDispatchError::Physical(format!("mixed loop publish: {error:?}"))
            })?;
        let entry = self
            .owner
            .borrow()
            .owned_numeric_f64_mixed_loop_entry(address)
            .map_err(|error| {
                NativeDispatchError::Physical(format!("mixed loop entry: {error:?}"))
            })?;
        self.installed = Some(entry);
        Ok(entry)
    }
}

fn range_proves_remainder_identity(
    seed: f64,
    end: usize,
    selection: MixedLoopSelection,
) -> Option<()> {
    let maximum_increment = selection
        .exceptional_increment
        .max(selection.ordinary_increment);
    let maximum = seed + maximum_increment * end as f64;
    (seed.is_finite() && seed >= 0.0 && maximum.is_finite() && maximum < selection.value_modulus)
        .then_some(())
}

pub(crate) fn select_mixed_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<MixedLoopSelection> {
    (start == 0 && entries.len() >= REGION_END).then_some(())?;
    cfg.region_control(start, REGION_END)?;
    let instructions = operation_window(entries)?;
    let constants = constants_and_operators(code, &instructions)?;
    bindings_match(code, &instructions)?;
    Some(MixedLoopSelection {
        state_slot: instructions[0].b,
        value_slot: instructions[2].a,
        index_slot: instructions[5].a,
        exceptional_increment: constants.exceptional_increment,
        ordinary_increment: constants.ordinary_increment,
        period: constants.period,
        value_modulus: constants.value_modulus,
    })
}

struct MixedConstants {
    exceptional_increment: f64,
    ordinary_increment: f64,
    period: usize,
    value_modulus: f64,
}

fn operation_window(entries: &[BaselineEntry]) -> Option<[Instruction; REGION_END]> {
    let instructions: [Instruction; REGION_END] = entries
        .get(..REGION_END)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let expected = [
        Opcode::LoadLocal,
        Opcode::GetN,
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
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::JumpIfFalse,
        Opcode::LoadConst,
        Opcode::Move,
        Opcode::Jump,
        Opcode::LoadConst,
        Opcode::Move,
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
        Opcode::Return,
        Opcode::LoadConst,
        Opcode::Return,
    ];
    instructions
        .iter()
        .zip(expected)
        .all(|(actual, expected)| {
            actual.opcode == expected
                || (expected == Opcode::GetN && actual.opcode == Opcode::GetNQuickened)
        })
        .then_some(instructions)
}

fn constants_and_operators(
    code: CodeView<'_>,
    i: &[Instruction; REGION_END],
) -> Option<MixedConstants> {
    undefined_constant(code, i[3])?;
    exact_number(code, i[4], 0.0)?;
    undefined_constant(code, i[6])?;
    let period = exact_positive_usize(number_constant(code, i[14])?)?;
    exact_number(code, i[16], 0.0)?;
    let exceptional_increment = number_constant(code, i[19])?;
    let ordinary_increment = number_constant(code, i[22])?;
    let value_modulus = number_constant(code, i[25])?;
    exact_number(code, i[30], 1.0)?;
    undefined_constant(code, i[37])?;
    for (at, operator) in [
        (10, crate::ops::BinaryOp::LessThan),
        (15, crate::ops::BinaryOp::Remainder),
        (17, crate::ops::BinaryOp::StrictEqual),
        (26, crate::ops::BinaryOp::Remainder),
        (31, crate::ops::BinaryOp::NumericAdd),
    ] {
        binary_operator(i[at], operator)?;
    }
    (exceptional_increment > 0.0 && ordinary_increment > 0.0 && value_modulus > 0.0).then_some(
        MixedConstants {
            exceptional_increment,
            ordinary_increment,
            period,
            value_modulus,
        },
    )
}

fn bindings_match(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    let state = i[0].b;
    let value = i[2].a;
    let index = i[5].a;
    (state != value && state != index && value != index).then_some(())?;
    (i[1].b == i[0].a && i[2].b == i[1].a && i[5].b == i[4].a).then_some(())?;
    (i[7].b == index && i[8].b == state && i[9].b == i[8].a).then_some(())?;
    (i[10].b == i[7].a && i[10].c == i[9].a && i[11].a == i[10].a).then_some(())?;
    (usize::from(i[11].b) == LOOP_EXIT && usize::from(i[34].a) == LOOP_HEADER).then_some(())?;
    (i[12].b == value && i[13].b == index && i[15].b == i[13].a && i[15].c == i[14].a)
        .then_some(())?;
    (i[17].b == i[15].a && i[17].c == i[16].a && i[18].a == i[17].a).then_some(())?;
    (usize::from(i[18].b) == 22 && i[20].a == i[23].a && usize::from(i[21].a) == 24)
        .then_some(())?;
    (i[20].b == i[19].a && i[23].b == i[22].a && i[24].b == i[12].a && i[24].c == i[20].a)
        .then_some(())?;
    (i[26].b == i[24].a && i[26].c == i[25].a && i[27].a == value && i[27].b == i[26].a)
        .then_some(())?;
    (i[28].b == i[26].a && i[29].b == index && i[31].b == i[29].a && i[31].c == i[30].a)
        .then_some(())?;
    (i[32].a == index && i[32].b == i[31].a && i[33].b == i[29].a).then_some(())?;
    (i[35].b == value && i[36].a == i[35].a && i[38].a == i[37].a).then_some(())?;
    code.metadata_at(1)?.name.as_deref()?;
    code.metadata_at(9)?.name.as_deref()?;
    Some(())
}

fn binary_operator(instruction: Instruction, expected: crate::ops::BinaryOp) -> Option<()> {
    (crate::ir::compact_binary_operator(instruction.flags) == Some(expected)).then_some(())
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
fn number_constant(code: CodeView<'_>, instruction: Instruction) -> Option<f64> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    Some(*value)
}
fn exact_positive_usize(value: f64) -> Option<usize> {
    (value.is_finite() && value >= 1.0 && value.fract() == 0.0 && value <= MAX_ITERATIONS as f64)
        .then_some(value as usize)
}
fn exact_bound(value: f64) -> Option<usize> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= MAX_ITERATIONS as f64)
        .then_some(value as usize)
}
