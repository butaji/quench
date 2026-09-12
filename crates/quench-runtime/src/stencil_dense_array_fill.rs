//! Guarded dense-Number fill loop with an ordered terminal read.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView, NativeDispatchError};
use std::{cell::RefCell, rc::Rc};

pub(crate) const REGION_END: usize = 35;
const LOOP_HEADER: usize = 4;
const LOOP_BACKEDGE: usize = 21;
const LOOP_EXIT: usize = 22;

const OPERATIONS: [Opcode; REGION_END] = [
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
    Opcode::Move,
    Opcode::LoadLocal,
    Opcode::Move,
    Opcode::LoadLocal,
    Opcode::ASetI,
    Opcode::Move,
    Opcode::LoadLocal,
    Opcode::LoadConst,
    Opcode::Binary,
    Opcode::StoreLocal,
    Opcode::Unary,
    Opcode::Jump,
    Opcode::LoadLocal,
    Opcode::Slow,
    Opcode::LoadConst,
    Opcode::AGetI,
    Opcode::LoadLocal,
    Opcode::Slow,
    Opcode::LoadLocal,
    Opcode::GetN,
    Opcode::LoadConst,
    Opcode::Sub,
    Opcode::AGetI,
    Opcode::Add,
    Opcode::Return,
];

#[derive(Clone, Copy)]
pub(crate) struct DenseFillSelection {
    array_slot: u16,
    value_slot: u16,
    index_slot: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DenseFillOutcome {
    Completed(f64),
    Resume { pc: usize },
}

pub(crate) struct NativeDenseFillPlan {
    selection: DenseFillSelection,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<crate::stencil_arena::DispatchEntry>,
}

impl NativeDenseFillPlan {
    pub(crate) fn new(
        selection: DenseFillSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.array_numeric_loops.then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::array_numeric_fill_loop_region_key(),
            crate::stencil_select::RegionAbi::ArrayNumericLoop,
        )?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        let site = crate::quickening::QuickeningSite::<4>::new(Opcode::ASetI);
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
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Result<Option<DenseFillOutcome>, NativeDispatchError> {
        let Some(value) = environment.get_number(self.selection.value_slot) else {
            return Ok(None);
        };
        environment
            .with_proven_array(self.selection.array_slot, |array| {
                self.execute_array(array, value, environment, context)
            })
            .unwrap_or(Ok(None))
    }

    fn execute_array(
        &mut self,
        array: &crate::value::ArrayData,
        value: f64,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Result<Option<DenseFillOutcome>, NativeDispatchError> {
        // Replacement resolution is part of the native-entry proof. The
        // current-representative check must precede even empty-array handling
        // so no stale array can reach a mutable backing view.
        if !crate::locals::array_word_is_current(array) {
            return Ok(None);
        }
        if array.is_empty() {
            if !array.can_fast_fill() {
                return Ok(None);
            }
            let mut empty = [];
            return self.execute_words(&mut empty, value, environment, context);
        }
        if !array.is_packed_ordinary() {
            return Ok(None);
        }
        let mut words = array.numeric_kernel_words_mut().ok_or_else(|| {
            NativeDispatchError::Physical("dense fill backing changed before entry".into())
        })?;
        // Mutable numeric-view preparation may widen the canonical element
        // kind. Capture the complete ownership stamp only after that guard
        // transition, before entering the native kernel.
        let backing = array.backing_identity();
        let outcome = self.execute_words(&mut words, value, environment, context)?;
        if !backing.is_current(array) {
            return Err(NativeDispatchError::committed(
                LOOP_BACKEDGE,
                "dense fill backing generation changed during native execution",
            ));
        }
        drop(words);
        Ok(outcome)
    }

    fn execute_words(
        &mut self,
        words: &mut [f64],
        value: f64,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Result<Option<DenseFillOutcome>, NativeDispatchError> {
        let mut native = native_context(words, value, context);
        let status = self.invoke(&mut native)?;
        let outcome = finish_native(status, &native, &words)?;
        if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
            context.clear_interrupt();
        }
        environment.set(
            self.selection.index_slot,
            crate::value::Value::Number(native.index as f64),
        );
        Ok(Some(outcome))
    }

    fn invoke(
        &mut self,
        context: &mut crate::vm::NativeArrayLoopContext,
    ) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        self.physical
            .invoke(entry, |call| {
                call((context as *mut crate::vm::NativeArrayLoopContext).cast())
            })
            .map_err(|error| NativeDispatchError::Physical(format!("dense fill invoke: {error:?}")))
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
                |pool, address| pool.owned_array_numeric_loop_entry(address),
            )
            .map_err(|error| NativeDispatchError::Physical(format!("dense fill entry: {error:?}")))
    }
}

fn native_context(
    words: &mut [f64],
    value: f64,
    context: &crate::vm::VmContext,
) -> crate::vm::NativeArrayLoopContext {
    crate::vm::NativeArrayLoopContext {
        data: words.as_mut_ptr(),
        len: words.len(),
        index: 0,
        end: words.len(),
        addend: value,
        result: value,
        interrupt: context.interrupt_flag(),
    }
}

fn finish_native(
    status: u64,
    native: &crate::vm::NativeArrayLoopContext,
    words: &[f64],
) -> Result<DenseFillOutcome, NativeDispatchError> {
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT && native.index < native.end {
        return Ok(DenseFillOutcome::Resume { pc: LOOP_HEADER });
    }
    let complete = status == crate::vm::NATIVE_DISPATCH_OK
        || (status == crate::vm::NATIVE_DISPATCH_INTERRUPT && native.index == native.end);
    if !complete || native.index != native.end {
        return Err(NativeDispatchError::committed(
            LOOP_BACKEDGE,
            "dense fill native loop returned incomplete progress",
        ));
    }
    let Some(first) = words.first() else {
        return Ok(DenseFillOutcome::Completed(f64::NAN));
    };
    Ok(DenseFillOutcome::Completed(*first + words[words.len() - 1]))
}

pub(crate) fn select_dense_fill(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<DenseFillSelection> {
    (start == 0 && entries.len() >= REGION_END).then_some(())?;
    cfg.region_control(start, REGION_END)?;
    let instructions = operation_window(entries)?;
    constants_match(code, &instructions)?;
    bindings_match(code, &instructions)?;
    Some(DenseFillSelection {
        array_slot: instructions[5].b,
        value_slot: instructions[13].b,
        index_slot: instructions[2].a,
    })
}

fn operation_window(entries: &[BaselineEntry]) -> Option<[Instruction; REGION_END]> {
    let instructions: [Instruction; REGION_END] = entries
        .get(..REGION_END)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    instructions
        .iter()
        .zip(OPERATIONS)
        .all(|(instruction, opcode)| opcode.matches_physical_contract(instruction.opcode))
        .then_some(instructions)
}

fn constants_match(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    number_constant(code, i[1], 0.0)?;
    number_constant(code, i[17], 1.0)?;
    number_constant(code, i[24], 0.0)?;
    number_constant(code, i[30], 1.0)
}

fn number_constant(code: CodeView<'_>, instruction: Instruction, expected: f64) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    (value.to_bits() == expected.to_bits()).then_some(())
}

fn bindings_match(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    let index = i[2].a;
    let array = i[5].b;
    (i[2].b == i[1].a && i[4].b == index && i[11].b == index).then_some(())?;
    (i[16].b == index && i[19].a == index && i[19].b == i[18].a).then_some(())?;
    (i[5].a == i[6].b && i[6].flags == crate::ir::GETN_LENGTH_FLAG).then_some(())?;
    (i[7].opcode.binary_operator(i[7].flags) == Some(crate::ops::BinaryOp::LessThan)
        && i[7].b == i[4].a
        && i[7].c == i[6].a
        && i[8].a == i[7].a)
        .then_some(())?;
    (usize::from(i[8].b) == LOOP_EXIT && usize::from(i[21].a) == LOOP_HEADER).then_some(())?;
    body_bindings(i, array, index)?;
    tail_bindings(code, i, array)
}

fn body_bindings(i: &[Instruction; REGION_END], array: u16, index: u16) -> Option<()> {
    (i[9].b == array && i[10].b == i[9].a && i[12].b == i[10].a).then_some(())?;
    (i[13].b != array && i[14].a == i[12].a && i[14].b == i[11].a && i[14].c == i[13].a)
        .then_some(())?;
    (i[16].b == index && i[18].b == i[16].a && i[18].c == i[17].a).then_some(())?;
    (i[18].opcode.binary_operator(i[18].flags) == Some(crate::ops::BinaryOp::NumericAdd))
        .then_some(())
}

fn tail_bindings(code: CodeView<'_>, i: &[Instruction; REGION_END], array: u16) -> Option<()> {
    (i[22].b == array && i[25].b == i[22].a && i[25].c == i[24].a).then_some(())?;
    (i[26].b == array && i[28].b == array && i[28].a == i[29].b).then_some(())?;
    (i[29].flags == crate::ir::GETN_LENGTH_FLAG
        && i[31].b == i[29].a
        && i[31].c == i[30].a
        && i[32].b == i[26].a
        && i[32].c == i[31].a)
        .then_some(())?;
    (i[33].b == i[25].a && i[33].c == i[32].a && i[34].a == i[33].a).then_some(())?;
    require_object(code, 23, i[22].a)?;
    require_object(code, 27, i[26].a)
}

fn require_object(code: CodeView<'_>, pc: usize, source: u16) -> Option<()> {
    matches!(code.cold_at(pc), Some(crate::ops::Op::RequireObjectCoercible { src }) if *src == source)
        .then_some(())
}
