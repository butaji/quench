//! Guarded int32 bitwise recurrence over a lowered counting loop.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView, NativeDispatchError};
use std::{cell::RefCell, rc::Rc};

pub(crate) const REGION_END: usize = 35;
const SEED_OFFSET: usize = 1;
const BOUND_OFFSET: usize = 9;
const LOOP_HEADER_OFFSET: usize = 7;
const LOOP_BACKEDGE_OFFSET: usize = 30;
const LOOP_EXIT_OFFSET: usize = 31;

#[derive(Clone, Copy)]
pub(crate) struct BitwiseLoopSelection {
    start: usize,
    seed_pc: usize,
    bound_pc: usize,
    loop_header_pc: usize,
    loop_backedge_pc: usize,
    region_end_pc: usize,
    state_slot: u16,
    value_slot: u16,
    index_slot: u16,
    left_shift: u32,
    right_shift: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BitwiseLoopOutcome {
    Completed(i32),
    Resume { pc: usize },
}

#[repr(C)]
struct BitwiseLoopContext {
    index: usize,
    end: usize,
    value: i32,
    left_shift: u32,
    right_shift: u32,
    interrupt: *const std::sync::atomic::AtomicBool,
}

pub(crate) struct NativeBitwiseLoopPlan {
    selection: BitwiseLoopSelection,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<crate::stencil_arena::DispatchEntry>,
}

impl NativeBitwiseLoopPlan {
    pub(crate) fn new(
        selection: BitwiseLoopSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.numeric_i32_bitwise_loops.then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::numeric_bitwise_loop_region_key(),
            crate::stencil_select::RegionAbi::NumericI32BitwiseLoop,
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
    ) -> Result<Option<BitwiseLoopOutcome>, NativeDispatchError> {
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
                Some((exact_i32(seed)?, exact_bound(end)?))
            })
            .flatten()
    }

    fn context(&self, value: i32, end: usize, vm: &crate::vm::VmContext) -> BitwiseLoopContext {
        BitwiseLoopContext {
            index: 0,
            end,
            value,
            left_shift: self.selection.left_shift,
            right_shift: self.selection.right_shift,
            interrupt: vm.interrupt_flag(),
        }
    }

    fn invoke(&mut self, context: &mut BitwiseLoopContext) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        self.physical
            .invoke(entry, |call| {
                call((context as *mut BitwiseLoopContext).cast())
            })
            .map_err(|error| {
                NativeDispatchError::Physical(format!("bitwise loop invoke: {error:?}"))
            })
    }

    fn finish(
        &self,
        status: u64,
        context: BitwiseLoopContext,
        environment: &crate::environment::Environment,
        vm: &crate::vm::VmContext,
    ) -> Result<BitwiseLoopOutcome, NativeDispatchError> {
        if status == crate::vm::NATIVE_DISPATCH_INTERRUPT && context.index < context.end {
            vm.clear_interrupt();
            self.commit(&context, environment);
            return Ok(BitwiseLoopOutcome::Resume {
                pc: self.selection.loop_header_pc,
            });
        }
        if status != crate::vm::NATIVE_DISPATCH_OK || context.index != context.end {
            return Err(NativeDispatchError::committed(
                self.selection.loop_backedge_pc,
                "bitwise recurrence returned incomplete progress",
            ));
        }
        Ok(BitwiseLoopOutcome::Completed(context.value))
    }

    fn commit(&self, context: &BitwiseLoopContext, environment: &crate::environment::Environment) {
        environment.set(
            self.selection.index_slot,
            crate::value::Value::Number(context.index as f64),
        );
        environment.set(
            self.selection.value_slot,
            crate::value::Value::Number(f64::from(context.value)),
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
                |pool, address| pool.owned_numeric_i32_bitwise_loop_entry(address),
            )
            .map_err(|error| {
                NativeDispatchError::Physical(format!("bitwise loop entry: {error:?}"))
            })
    }

    pub(crate) const fn start_pc(&self) -> usize {
        self.selection.start
    }

    pub(crate) const fn region_end_pc(&self) -> usize {
        self.selection.region_end_pc
    }
}

pub(crate) fn select_bitwise_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<BitwiseLoopSelection> {
    let end = start.checked_add(REGION_END)?;
    cfg.region_control(start, end)?;
    let instructions = operation_window(entries, start)?;
    let (left_shift, right_shift) = constants_and_operators(code, &instructions)?;
    bindings_match(code, &instructions, start)?;
    Some(BitwiseLoopSelection {
        start,
        seed_pc: start.checked_add(SEED_OFFSET)?,
        bound_pc: start.checked_add(BOUND_OFFSET)?,
        loop_header_pc: start.checked_add(LOOP_HEADER_OFFSET)?,
        loop_backedge_pc: start.checked_add(LOOP_BACKEDGE_OFFSET)?,
        region_end_pc: end,
        state_slot: instructions[0].b,
        value_slot: instructions[2].a,
        index_slot: instructions[5].a,
        left_shift,
        right_shift,
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
        Opcode::Binary,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::Binary,
        Opcode::LoadLocal,
        Opcode::Binary,
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
        .all(|(actual, expected)| expected.matches_physical_contract(actual.opcode))
        .then_some(instructions)
}

fn constants_and_operators(
    code: CodeView<'_>,
    i: &[Instruction; REGION_END],
) -> Option<(u32, u32)> {
    undefined_constant(code, i[3])?;
    exact_number(code, i[4], 0.0)?;
    undefined_constant(code, i[6])?;
    let left = exact_shift(number_constant(code, i[13])?)?;
    let right = exact_shift(number_constant(code, i[16])?)?;
    exact_number(code, i[21], 0.0)?;
    exact_number(code, i[26], 1.0)?;
    undefined_constant(code, i[33])?;
    for (at, operator) in [
        (10, crate::ops::BinaryOp::LessThan),
        (14, crate::ops::BinaryOp::ShiftLeft),
        (17, crate::ops::BinaryOp::ShiftRightZeroFill),
        (18, crate::ops::BinaryOp::BitwiseXor),
        (20, crate::ops::BinaryOp::BitwiseXor),
        (22, crate::ops::BinaryOp::BitwiseOr),
        (27, crate::ops::BinaryOp::NumericAdd),
    ] {
        binary_operator(i[at], operator)?;
    }
    (crate::ir::compact_unary_operator(i[29].flags) == Some(crate::ops::UnaryOp::ToNumeric))
        .then_some((left, right))
}

fn bindings_match(code: CodeView<'_>, i: &[Instruction; REGION_END], start: usize) -> Option<()> {
    let state = i[0].b;
    let value = i[2].a;
    let index = i[5].a;
    (state != value && state != index && value != index).then_some(())?;
    (i[1].b == i[0].a && i[2].b == i[1].a && i[5].b == i[4].a).then_some(())?;
    (i[7].b == index && i[8].b == state && i[9].b == i[8].a).then_some(())?;
    (i[10].b == i[7].a && i[10].c == i[9].a && i[11].a == i[10].a).then_some(())?;
    (usize::from(i[11].b) == start.checked_add(LOOP_EXIT_OFFSET)?
        && usize::from(i[30].a) == start.checked_add(LOOP_HEADER_OFFSET)?
        && usize::from(i[30].a) < start.checked_add(LOOP_BACKEDGE_OFFSET)?
        && usize::from(i[11].b) < start.checked_add(REGION_END)?)
    .then_some(())?;
    (i[12].b == value && i[14].b == i[12].a && i[14].c == i[13].a).then_some(())?;
    (i[15].b == value && i[17].b == i[15].a && i[17].c == i[16].a).then_some(())?;
    (i[18].b == i[14].a && i[18].c == i[17].a).then_some(())?;
    (i[19].b == index && i[20].b == i[18].a && i[20].c == i[19].a).then_some(())?;
    (i[22].b == i[20].a && i[22].c == i[21].a && i[23].a == value).then_some(())?;
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

fn exact_shift(value: f64) -> Option<u32> {
    (value.is_finite() && value.fract() == 0.0 && (0.0..32.0).contains(&value))
        .then_some(value as u32)
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

#[cfg(test)]
mod tests {
    use super::{operation_window, REGION_END};
    use crate::ir::{Instruction, Opcode};
    use crate::machine::BaselineEntry;

    #[test]
    fn operation_window_is_relative_to_admitted_start() {
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
            Opcode::LoadConst,
            Opcode::Binary,
            Opcode::LoadLocal,
            Opcode::LoadConst,
            Opcode::Binary,
            Opcode::Binary,
            Opcode::LoadLocal,
            Opcode::Binary,
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
        let start = 4;
        let mut entries = (0..start)
            .map(|_| BaselineEntry {
                instruction: Instruction {
                    opcode: Opcode::Throw,
                    flags: 0,
                    a: 0,
                    b: 0,
                    c: 0,
                },
                control: crate::ir::ControlOperands::Throw { source: 0 },
            })
            .collect::<Vec<_>>();
        entries.extend(expected.iter().copied().map(|opcode| {
            let instruction = Instruction {
                opcode,
                flags: 0,
                a: 0,
                b: 0,
                c: 0,
            };
            BaselineEntry {
                control: opcode.control_operands(instruction),
                instruction,
            }
        }));
        let window = operation_window(&entries, start).expect("relative loop window");
        assert_eq!(window.len(), REGION_END);
        assert_eq!(window[0].opcode, Opcode::LoadLocal);
        assert_eq!(window[REGION_END - 1].opcode, Opcode::Return);
    }
}
