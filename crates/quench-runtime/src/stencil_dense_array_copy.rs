//! Guarded dense-Number copy loop and terminal two-element reduction.
//!
//! Admission proves the ordinary residual CFG, operand wiring, distinct plain
//! numeric backings, and nonescaping induction slot before native entry. The
//! ARM64 body copies in source order, publishes progress, polls interrupts,
//! and takes its backedge without a Rust operation bridge.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView, NativeDispatchError};
use std::{cell::RefCell, rc::Rc};

pub(crate) const REGION_END: usize = 38;
const LOOP_HEADER: usize = 4;
const LOOP_BACKEDGE: usize = 24;
const LOOP_EXIT: usize = 25;

const OPERATIONS: [Opcode; REGION_END] = [
    Opcode::LoadConst, Opcode::LoadConst, Opcode::StoreLocal, Opcode::LoadConst,
    Opcode::LoadLocal, Opcode::LoadLocal, Opcode::GetN, Opcode::Binary,
    Opcode::JumpIfFalse, Opcode::LoadLocal, Opcode::Move, Opcode::LoadLocal,
    Opcode::Move, Opcode::LoadLocal, Opcode::Slow, Opcode::LoadLocal,
    Opcode::AGetI, Opcode::ASetI, Opcode::Move, Opcode::LoadLocal,
    Opcode::LoadConst, Opcode::Binary, Opcode::StoreLocal, Opcode::Unary,
    Opcode::Jump, Opcode::LoadLocal, Opcode::Slow, Opcode::LoadConst,
    Opcode::AGetI, Opcode::LoadLocal, Opcode::Slow, Opcode::LoadLocal,
    Opcode::GetN, Opcode::LoadConst, Opcode::Sub, Opcode::AGetI, Opcode::Add,
    Opcode::Return,
];

#[repr(C)]
pub(crate) struct NativeArrayCopyContext {
    source: *const f64,
    destination: *mut f64,
    len: usize,
    index: usize,
    interrupt: *const std::sync::atomic::AtomicBool,
}

const _: () = {
    assert!(std::mem::align_of::<NativeArrayCopyContext>() == 8);
    assert!(std::mem::offset_of!(NativeArrayCopyContext, source) == 0);
    assert!(std::mem::offset_of!(NativeArrayCopyContext, destination) == 8);
    assert!(std::mem::offset_of!(NativeArrayCopyContext, len) == 16);
    assert!(std::mem::offset_of!(NativeArrayCopyContext, index) == 24);
    assert!(std::mem::offset_of!(NativeArrayCopyContext, interrupt) == 32);
    assert!(std::mem::size_of::<NativeArrayCopyContext>() == 40);
};

#[derive(Clone, Copy)]
pub(crate) struct DenseCopySelection {
    source_slot: u16,
    target_slot: u16,
    index_slot: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DenseCopyOutcome {
    Completed(f64),
    Resume { pc: usize },
}

pub(crate) struct NativeDenseCopyPlan {
    selection: DenseCopySelection,
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

impl NativeDenseCopyPlan {
    pub(crate) fn new(
        selection: DenseCopySelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.array_numeric_loops.then_some(())?;
        let view = crate::stencil_select::select_physical(
            crate::stencil_select::dense_numeric_copy_loop_region_key(),
        )?;
        (view.abi == crate::stencil_select::RegionAbi::ArrayCopyLoop
            && view.executable && view.stencil.validate()).then_some(())?;
        Some(Self {
            selection,
            owner,
            image: region_image(view),
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
        })
    }

    pub(crate) fn execute(
        &mut self,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Result<Option<DenseCopyOutcome>, NativeDispatchError> {
        if !environment.can_elide_terminal_store(self.selection.index_slot) {
            return Ok(None);
        }
        environment
            .with_proven_array(self.selection.source_slot, |source| {
                environment.with_proven_array(self.selection.target_slot, |target| {
                    self.execute_arrays(source, target, environment, context)
                })
            })
            .flatten()
            .unwrap_or(Ok(None))
    }

    fn execute_arrays(
        &mut self,
        source: &crate::value::ArrayData,
        target: &crate::value::ArrayData,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Result<Option<DenseCopyOutcome>, NativeDispatchError> {
        if !copy_guards_hold(source, target) {
            return Ok(None);
        }
        let source_words = source.numeric_kernel_words().ok_or_else(|| {
            NativeDispatchError::Physical("dense copy source backing changed".into())
        })?;
        let mut target_words = target.numeric_kernel_words_mut().ok_or_else(|| {
            NativeDispatchError::Physical("dense copy target backing changed".into())
        })?;
        let mut native = native_context(&source_words, &mut target_words, context);
        let status = self.invoke(&mut native)?;
        let outcome = finish_native(status, &native, &target_words)?;
        if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
            context.clear_interrupt();
        }
        drop(target_words);
        drop(source_words);
        if let DenseCopyOutcome::Resume { .. } = outcome {
            environment.set(
                self.selection.index_slot,
                crate::value::Value::Number(native.index as f64),
            );
        }
        Ok(Some(outcome))
    }

    fn invoke(&mut self, context: &mut NativeArrayCopyContext) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        let lease = crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry)
            .map_err(|error| NativeDispatchError::Physical(format!("dense copy lease: {error:?}")))?;
        lease.invoke(|call| call((context as *mut NativeArrayCopyContext).cast()))
            .map_err(|error| NativeDispatchError::Physical(format!("dense copy invoke: {error:?}")))
    }

    fn entry(&mut self) -> Result<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>, NativeDispatchError> {
        if let Some(entry) = self.installed {
            if self.owner.borrow().entry_token_is_live(entry) {
                return Ok(entry);
            }
            self.installed = None;
        }
        let address = self.owner.borrow_mut()
            .publish_region_image_or_get(&mut self.cache, &self.image)
            .map_err(|error| NativeDispatchError::Physical(format!("dense copy publish: {error:?}")))?;
        let entry = self.owner.borrow().owned_array_copy_loop_entry(address)
            .map_err(|error| NativeDispatchError::Physical(format!("dense copy entry: {error:?}")))?;
        self.installed = Some(entry);
        Ok(entry)
    }

    pub(crate) fn route() -> impl Iterator<Item = &'static str> {
        ["AGetI", "ASetI", "AddConst", "Jump", "Return"].into_iter()
    }
}

fn copy_guards_hold(source: &crate::value::ArrayData, target: &crate::value::ArrayData) -> bool {
    source.identity() != target.identity()
        && source.is_plain_dense_access()
        && target.is_plain_dense_access()
        && source.is_dense_numeric_data()
        && target.is_dense_numeric_data()
        && target.len() != 0
        && source.len() <= target.len()
}

fn native_context(
    source: &[f64],
    target: &mut [f64],
    context: &crate::vm::VmContext,
) -> NativeArrayCopyContext {
    NativeArrayCopyContext {
        source: source.as_ptr(),
        destination: target.as_mut_ptr(),
        len: source.len(),
        index: 0,
        interrupt: context.interrupt_flag(),
    }
}

fn finish_native(
    status: u64,
    native: &NativeArrayCopyContext,
    target: &[f64],
) -> Result<DenseCopyOutcome, NativeDispatchError> {
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT && native.index < native.len {
        return Ok(DenseCopyOutcome::Resume { pc: LOOP_HEADER });
    }
    let complete = status == crate::vm::NATIVE_DISPATCH_OK
        || (status == crate::vm::NATIVE_DISPATCH_INTERRUPT && native.index == native.len);
    if !complete || native.index != native.len {
        return Err(NativeDispatchError::committed(
            LOOP_BACKEDGE,
            "dense copy native loop returned incomplete progress",
        ));
    }
    let first = target.first().ok_or_else(|| {
        NativeDispatchError::committed(LOOP_EXIT, "dense copy target is empty")
    })?;
    Ok(DenseCopyOutcome::Completed(*first + target[target.len() - 1]))
}

fn region_image(view: crate::stencil_select::PhysicalStencilView) -> crate::stencil_region_layout::VerifiedRegionImage {
    let identity = crate::stencil_region_layout::RegionImageIdentity {
        key: view.key,
        cache_signature: byte_fingerprint(view.stencil.bytes),
        abi: view.abi,
    };
    crate::stencil_region_layout::VerifiedRegionImage::from_composed(identity, view.stencil.bytes.to_vec())
}

fn byte_fingerprint(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x1000_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(PRIME))
}

pub(crate) fn select_dense_copy(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<DenseCopySelection> {
    if start != 0 || entries.len() < REGION_END {
        return None;
    }
    cfg.region_control(start, REGION_END)?;
    let instructions = operation_window(entries)?;
    constants_match(code, &instructions)?;
    bindings_match(code, &instructions)?;
    Some(DenseCopySelection {
        source_slot: instructions[5].b,
        target_slot: instructions[9].b,
        index_slot: instructions[2].a,
    })
}

fn operation_window(entries: &[BaselineEntry]) -> Option<[Instruction; REGION_END]> {
    let instructions: [Instruction; REGION_END] = entries.get(..REGION_END)?
        .iter().map(|entry| entry.instruction).collect::<Vec<_>>().try_into().ok()?;
    instructions.iter().zip(OPERATIONS)
        .all(|(instruction, opcode)| instruction.opcode == opcode).then_some(instructions)
}

fn constants_match(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    number_constant(code, i[1], 0.0)?;
    number_constant(code, i[20], 1.0)?;
    number_constant(code, i[27], 0.0)?;
    number_constant(code, i[33], 1.0)
}

fn number_constant(code: CodeView<'_>, instruction: Instruction, expected: f64) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else { return None };
    (value.to_bits() == expected.to_bits()).then_some(())
}

fn bindings_match(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    let index = i[2].a;
    let source = i[5].b;
    let target = i[9].b;
    (i[2].b == i[1].a && i[4].b == index && i[11].b == index).then_some(())?;
    (i[15].b == index && i[19].b == index && i[22].a == index).then_some(())?;
    (i[5].a == i[6].b && i[6].flags == crate::ir::GETN_LENGTH_FLAG).then_some(())?;
    (i[7].b == i[4].a && i[7].c == i[6].a && i[8].a == i[7].a).then_some(())?;
    (usize::from(i[8].b) == LOOP_EXIT && usize::from(i[24].a) == LOOP_HEADER).then_some(())?;
    body_bindings_match(code, i, source, target)?;
    tail_bindings_match(code, i, target)
}

fn body_bindings_match(code: CodeView<'_>, i: &[Instruction; REGION_END], source: u16, target: u16) -> Option<()> {
    (i[9].b == target && i[13].b == source).then_some(())?;
    (i[10].b == i[9].a && i[12].b == i[10].a).then_some(())?;
    require_object(code, 14, i[13].a)?;
    (i[16].b == i[13].a && i[16].c == i[15].a).then_some(())?;
    (i[17].a == i[12].a && i[17].b == i[11].a && i[17].c == i[16].a).then_some(())?;
    (i[21].b == i[19].a && i[21].c == i[20].a && i[22].b == i[21].a).then_some(())
}

fn tail_bindings_match(code: CodeView<'_>, i: &[Instruction; REGION_END], target: u16) -> Option<()> {
    (i[25].b == target && i[28].b == i[25].a && i[28].c == i[27].a).then_some(())?;
    require_object(code, 26, i[25].a)?;
    (i[29].b == target && i[31].b == target && i[31].a == i[32].b).then_some(())?;
    require_object(code, 30, i[29].a)?;
    (i[32].flags == crate::ir::GETN_LENGTH_FLAG && i[34].b == i[32].a && i[34].c == i[33].a).then_some(())?;
    (i[35].b == i[29].a && i[35].c == i[34].a).then_some(())?;
    (i[36].b == i[28].a && i[36].c == i[35].a && i[37].a == i[36].a).then_some(())
}

fn require_object(code: CodeView<'_>, pc: usize, source: u16) -> Option<()> {
    matches!(code.cold_at(pc), Some(crate::ops::Op::RequireObjectCoercible { src }) if *src == source).then_some(())
}
