//! Guarded floating-point recurrence over one canonical lowered counting loop.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView, NativeDispatchError};
use std::{cell::RefCell, rc::Rc};

pub(crate) const REGION_END: usize = 35;
const SEED_OFFSET: usize = 1;
const BOUND_OFFSET: usize = 11;
const LOOP_HEADER_OFFSET: usize = 9;
const LOOP_BACKEDGE_OFFSET: usize = 30;
const LOOP_EXIT_OFFSET: usize = 31;

#[derive(Clone, Copy)]
pub(crate) struct FloatingLoopSelection {
    start: usize,
    seed_pc: usize,
    bound_pc: usize,
    loop_header_pc: usize,
    loop_backedge_pc: usize,
    region_end_pc: usize,
    state_slot: u16,
    value_slot: u16,
    index_slot: u16,
    seed_divisor: f64,
    multiplier: f64,
    modulus: usize,
    term_divisor: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FloatingLoopOutcome {
    Completed(f64),
    Resume { pc: usize },
}

#[repr(C)]
struct FloatingLoopContext {
    index: usize,
    end: usize,
    value: f64,
    multiplier: f64,
    divisor: f64,
    modulus: usize,
    interrupt: *const std::sync::atomic::AtomicBool,
}

pub(crate) struct NativeFloatingLoopPlan {
    selection: FloatingLoopSelection,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<crate::stencil_arena::DispatchEntry>,
}

impl NativeFloatingLoopPlan {
    pub(crate) fn new(
        selection: FloatingLoopSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.numeric_f64_loops.then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::numeric_floating_loop_region_key(),
            crate::stencil_select::RegionAbi::NumericF64Loop,
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
    ) -> Result<Option<FloatingLoopOutcome>, NativeDispatchError> {
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
                let seed =
                    crate::vm::cached_own_property_number(code, self.selection.seed_pc, object)?;
                let end =
                    crate::vm::cached_own_property_number(code, self.selection.bound_pc, object)?;
                Some((seed / self.selection.seed_divisor, exact_bound(end)?))
            })
            .flatten()
    }

    fn context(&self, value: f64, end: usize, vm: &crate::vm::VmContext) -> FloatingLoopContext {
        FloatingLoopContext {
            index: 0,
            end,
            value,
            multiplier: self.selection.multiplier,
            divisor: self.selection.term_divisor,
            modulus: self.selection.modulus,
            interrupt: vm.interrupt_flag(),
        }
    }

    fn invoke(&mut self, context: &mut FloatingLoopContext) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        self.physical
            .invoke(entry, |call| {
                call((context as *mut FloatingLoopContext).cast())
            })
            .map_err(|error| {
                NativeDispatchError::Physical(format!("floating loop invoke: {error:?}"))
            })
    }

    fn finish(
        &self,
        status: u64,
        context: FloatingLoopContext,
        environment: &crate::environment::Environment,
        vm: &crate::vm::VmContext,
    ) -> Result<FloatingLoopOutcome, NativeDispatchError> {
        if status == crate::vm::NATIVE_DISPATCH_INTERRUPT && context.index < context.end {
            vm.clear_interrupt();
            self.commit(&context, environment);
            return Ok(FloatingLoopOutcome::Resume {
                pc: self.selection.loop_header_pc,
            });
        }
        if status != crate::vm::NATIVE_DISPATCH_OK || context.index != context.end {
            return Err(NativeDispatchError::committed(
                self.selection.loop_backedge_pc,
                "floating recurrence returned incomplete progress",
            ));
        }
        Ok(FloatingLoopOutcome::Completed(context.value))
    }

    fn commit(&self, context: &FloatingLoopContext, environment: &crate::environment::Environment) {
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
        let image = &self.image;
        self.physical
            .entry(
                |owner, cache| owner.borrow_mut().publish_region_image_or_get(cache, image),
                |pool, address| pool.owned_numeric_f64_loop_entry(address),
            )
            .map_err(|error| {
                NativeDispatchError::Physical(format!("floating loop entry: {error:?}"))
            })
    }

    pub(crate) const fn start_pc(&self) -> usize {
        self.selection.start
    }

    pub(crate) const fn region_end_pc(&self) -> usize {
        self.selection.region_end_pc
    }
}

pub(crate) fn select_floating_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<FloatingLoopSelection> {
    let end = start.checked_add(REGION_END)?;
    cfg.region_control(start, end)?;
    let instructions = operation_window(entries, start)?;
    let constants = constants_and_operators(code, &instructions)?;
    bindings_match(code, &instructions, start)?;
    Some(FloatingLoopSelection {
        start,
        seed_pc: start.checked_add(SEED_OFFSET)?,
        bound_pc: start.checked_add(BOUND_OFFSET)?,
        loop_header_pc: start.checked_add(LOOP_HEADER_OFFSET)?,
        loop_backedge_pc: start.checked_add(LOOP_BACKEDGE_OFFSET)?,
        region_end_pc: end,
        state_slot: instructions[0].b,
        value_slot: instructions[4].a,
        index_slot: instructions[7].a,
        seed_divisor: constants.seed_divisor,
        multiplier: constants.multiplier,
        modulus: constants.modulus,
        term_divisor: constants.term_divisor,
    })
}

struct FloatingConstants {
    seed_divisor: f64,
    multiplier: f64,
    modulus: usize,
    term_divisor: f64,
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
        Opcode::LoadConst,
        Opcode::Div,
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
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::LoadConst,
        Opcode::Div,
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
    instructions
        .iter()
        .zip(expected)
        .all(|(actual, expected)| expected.matches_physical_contract(actual.opcode))
        .then_some(instructions)
}

fn constants_and_operators(
    code: CodeView<'_>,
    i: &[Instruction; REGION_END],
) -> Option<FloatingConstants> {
    let seed_divisor = number_constant(code, i[2])?;
    undefined_constant(code, i[5])?;
    exact_number(code, i[6], 0.0)?;
    undefined_constant(code, i[8])?;
    let multiplier = number_constant(code, i[15])?;
    let modulus = exact_positive_usize(number_constant(code, i[18])?)?;
    let term_divisor = number_constant(code, i[20])?;
    exact_number(code, i[26], 1.0)?;
    undefined_constant(code, i[33])?;
    (seed_divisor != 0.0 && term_divisor != 0.0).then_some(())?;
    binary_operator(i[12], crate::ops::BinaryOp::LessThan)?;
    binary_operator(i[19], crate::ops::BinaryOp::Remainder)?;
    binary_operator(i[27], crate::ops::BinaryOp::NumericAdd)?;
    (crate::ir::compact_unary_operator(i[29].flags) == Some(crate::ops::UnaryOp::ToNumeric))
        .then_some(())?;
    Some(FloatingConstants {
        seed_divisor,
        multiplier,
        modulus,
        term_divisor,
    })
}

fn bindings_match(code: CodeView<'_>, i: &[Instruction; REGION_END], start: usize) -> Option<()> {
    let state = i[0].b;
    let value = i[4].a;
    let index = i[7].a;
    (state != value && state != index && value != index).then_some(())?;
    (i[1].b == i[0].a && i[3].b == i[1].a && i[3].c == i[2].a).then_some(())?;
    (i[4].b == i[3].a && i[7].b == i[6].a).then_some(())?;
    (i[9].b == index && i[10].b == state && i[11].b == i[10].a).then_some(())?;
    (i[12].b == i[9].a && i[12].c == i[11].a && i[13].a == i[12].a).then_some(())?;
    (usize::from(i[13].b) == start.checked_add(LOOP_EXIT_OFFSET)?
        && usize::from(i[30].a) == start.checked_add(LOOP_HEADER_OFFSET)?
        && usize::from(i[30].a) < start.checked_add(LOOP_BACKEDGE_OFFSET)?
        && usize::from(i[13].b) < start.checked_add(REGION_END)?)
    .then_some(())?;
    (i[14].b == value && i[16].b == i[14].a && i[16].c == i[15].a).then_some(())?;
    (i[17].b == index && i[19].b == i[17].a && i[19].c == i[18].a).then_some(())?;
    (i[21].b == i[19].a && i[21].c == i[20].a).then_some(())?;
    (i[22].b == i[16].a && i[22].c == i[21].a && i[23].a == value).then_some(())?;
    (i[23].b == i[22].a && i[24].b == i[22].a).then_some(())?;
    (i[25].b == index && i[27].b == i[25].a && i[27].c == i[26].a).then_some(())?;
    (i[28].a == index && i[28].b == i[27].a && i[29].b == i[25].a).then_some(())?;
    (i[31].b == value && i[32].a == i[31].a && i[34].a == i[33].a).then_some(())?;
    code.metadata_at(start.checked_add(SEED_OFFSET)?)?
        .name
        .as_deref()?;
    code.metadata_at(start.checked_add(BOUND_OFFSET)?)?
        .name
        .as_deref()?;
    Some(())
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

fn number_constant(code: CodeView<'_>, instruction: Instruction) -> Option<f64> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    Some(*value)
}

fn exact_positive_usize(value: f64) -> Option<usize> {
    exact_usize(value).filter(|value| *value >= 1)
}

fn exact_bound(value: f64) -> Option<usize> {
    exact_usize(value)
}

fn exact_usize(value: f64) -> Option<usize> {
    (value.is_finite() && value >= 0.0 && value < usize::MAX as f64 && value.fract() == 0.0)
        .then_some(())?;
    let integer = value as usize;
    (integer as f64 == value).then_some(integer)
}
