//! Guarded ordered dense-Number reduction.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView, NativeDispatchError};
use std::{cell::RefCell, rc::Rc};

pub(crate) const REGION_END: usize = 27;
const LOOP_HEADER: usize = 6;
const LOOP_BACKEDGE: usize = 24;
const OPERATIONS: [Opcode; REGION_END] = [
    Opcode::LoadConst, Opcode::StoreLocal, Opcode::LoadConst, Opcode::LoadConst,
    Opcode::StoreLocal, Opcode::LoadConst, Opcode::LoadLocal, Opcode::LoadLocal,
    Opcode::GetN, Opcode::Binary, Opcode::JumpIfFalse, Opcode::LoadLocal,
    Opcode::LoadLocal, Opcode::Slow, Opcode::LoadLocal, Opcode::AGetI, Opcode::Add,
    Opcode::StoreLocal, Opcode::Move, Opcode::LoadLocal, Opcode::LoadConst,
    Opcode::Binary, Opcode::StoreLocal, Opcode::Unary, Opcode::Jump,
    Opcode::LoadLocal, Opcode::Return,
];

#[repr(C)]
pub(crate) struct NativeReductionContext {
    source: *const f64,
    len: usize,
    index: usize,
    total: f64,
    interrupt: *const std::sync::atomic::AtomicBool,
}

const _: () = {
    assert!(std::mem::align_of::<NativeReductionContext>() == 8);
    assert!(std::mem::offset_of!(NativeReductionContext, source) == 0);
    assert!(std::mem::offset_of!(NativeReductionContext, len) == 8);
    assert!(std::mem::offset_of!(NativeReductionContext, index) == 16);
    assert!(std::mem::offset_of!(NativeReductionContext, total) == 24);
    assert!(std::mem::offset_of!(NativeReductionContext, interrupt) == 32);
    assert!(std::mem::size_of::<NativeReductionContext>() == 40);
};

#[derive(Clone, Copy)]
pub(crate) struct ReductionSelection {
    array_slot: u16,
    total_slot: u16,
    index_slot: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ReductionOutcome {
    Completed(f64),
    Resume { pc: usize },
}

pub(crate) struct NativeReductionPlan {
    selection: ReductionSelection,
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

impl NativeReductionPlan {
    pub(crate) fn new(
        selection: ReductionSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.array_numeric_loops.then_some(())?;
        let view = crate::stencil_select::select_physical(
            crate::stencil_select::ordered_f64_reduction_loop_region_key(),
        )?;
        (view.abi == crate::stencil_select::RegionAbi::ArrayReductionLoop
            && view.executable && view.stencil.validate()).then_some(())?;
        Some(Self {
            selection, owner, image: region_image(view),
            cache: crate::stencil_select::RenderedRegionCache::new(), installed: None,
        })
    }

    pub(crate) fn execute(
        &mut self,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Result<Option<ReductionOutcome>, NativeDispatchError> {
        if !environment.can_elide_terminal_store(self.selection.index_slot)
            || !environment.can_elide_terminal_store(self.selection.total_slot)
        {
            return Ok(None);
        }
        environment.with_proven_array(self.selection.array_slot, |array| {
            self.execute_array(array, environment, context)
        }).unwrap_or(Ok(None))
    }

    fn execute_array(
        &mut self,
        array: &crate::value::ArrayData,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Result<Option<ReductionOutcome>, NativeDispatchError> {
        if !array.is_packed_data() || !array.is_dense_numeric_data() {
            return Ok(None);
        }
        let words = array.numeric_kernel_words().ok_or_else(|| {
            NativeDispatchError::Physical("ordered reduction backing changed".into())
        })?;
        let mut native = NativeReductionContext {
            source: words.as_ptr(), len: words.len(), index: 0, total: 0.0,
            interrupt: context.interrupt_flag(),
        };
        let status = self.invoke(&mut native)?;
        let outcome = finish_native(status, &native)?;
        if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
            context.clear_interrupt();
        }
        drop(words);
        if let ReductionOutcome::Resume { .. } = outcome {
            environment.set(self.selection.index_slot, crate::value::Value::Number(native.index as f64));
            environment.set(self.selection.total_slot, crate::value::Value::Number(native.total));
        }
        Ok(Some(outcome))
    }

    fn invoke(&mut self, context: &mut NativeReductionContext) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        let lease = crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry)
            .map_err(|error| NativeDispatchError::Physical(format!("reduction lease: {error:?}")))?;
        lease.invoke(|call| call((context as *mut NativeReductionContext).cast()))
            .map_err(|error| NativeDispatchError::Physical(format!("reduction invoke: {error:?}")))
    }

    fn entry(&mut self) -> Result<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>, NativeDispatchError> {
        if let Some(entry) = self.installed {
            if self.owner.borrow().entry_token_is_live(entry) { return Ok(entry) }
            self.installed = None;
        }
        let address = self.owner.borrow_mut()
            .publish_region_image_or_get(&mut self.cache, &self.image)
            .map_err(|error| NativeDispatchError::Physical(format!("reduction publish: {error:?}")))?;
        let entry = self.owner.borrow().owned_array_reduction_loop_entry(address)
            .map_err(|error| NativeDispatchError::Physical(format!("reduction entry: {error:?}")))?;
        self.installed = Some(entry);
        Ok(entry)
    }

    pub(crate) fn route() -> impl Iterator<Item = &'static str> {
        ["AGetI", "Add", "AddConst", "Jump", "Return"].into_iter()
    }
}

fn finish_native(status: u64, native: &NativeReductionContext) -> Result<ReductionOutcome, NativeDispatchError> {
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT && native.index < native.len {
        return Ok(ReductionOutcome::Resume { pc: LOOP_HEADER });
    }
    let complete = status == crate::vm::NATIVE_DISPATCH_OK
        || (status == crate::vm::NATIVE_DISPATCH_INTERRUPT && native.index == native.len);
    if !complete || native.index != native.len {
        return Err(NativeDispatchError::committed(LOOP_BACKEDGE, "ordered reduction returned incomplete progress"));
    }
    Ok(ReductionOutcome::Completed(native.total))
}

fn region_image(view: crate::stencil_select::PhysicalStencilView) -> crate::stencil_region_layout::VerifiedRegionImage {
    let identity = crate::stencil_region_layout::RegionImageIdentity {
        key: view.key, cache_signature: fingerprint(view.stencil.bytes), abi: view.abi,
    };
    crate::stencil_region_layout::VerifiedRegionImage::from_composed(identity, view.stencil.bytes.to_vec())
}

fn fingerprint(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x1000_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(PRIME))
}

pub(crate) fn select_reduction(
    code: CodeView<'_>, entries: &[BaselineEntry], cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<ReductionSelection> {
    if start != 0 || entries.len() < REGION_END { return None }
    cfg.region_control(start, REGION_END)?;
    let i = operation_window(entries)?;
    constants_match(code, &i)?;
    bindings_match(code, &i)?;
    Some(ReductionSelection { total_slot: i[1].a, array_slot: i[7].b, index_slot: i[4].a })
}

fn operation_window(entries: &[BaselineEntry]) -> Option<[Instruction; REGION_END]> {
    let instructions: [Instruction; REGION_END] = entries.get(..REGION_END)?
        .iter().map(|entry| entry.instruction).collect::<Vec<_>>().try_into().ok()?;
    instructions.iter().zip(OPERATIONS)
        .all(|(instruction, opcode)| instruction.opcode == opcode).then_some(instructions)
}

fn constants_match(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    number_constant(code, i[0], 0.0)?;
    number_constant(code, i[3], 0.0)?;
    number_constant(code, i[20], 1.0)
}

fn number_constant(code: CodeView<'_>, instruction: Instruction, expected: f64) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else { return None };
    (value.to_bits() == expected.to_bits()).then_some(())
}

fn bindings_match(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    let total = i[1].a;
    let index = i[4].a;
    let array = i[7].b;
    (i[1].b == i[0].a && i[4].b == i[3].a && i[6].b == index).then_some(())?;
    (i[7].a == i[8].b && i[8].flags == crate::ir::GETN_LENGTH_FLAG).then_some(())?;
    (i[9].b == i[6].a && i[9].c == i[8].a && i[10].a == i[9].a).then_some(())?;
    (usize::from(i[10].b) == 25 && usize::from(i[24].a) == LOOP_HEADER).then_some(())?;
    (i[11].b == total && i[12].b == array && i[14].b == index).then_some(())?;
    require_object(code, 13, i[12].a)?;
    (i[15].b == i[12].a && i[15].c == i[14].a).then_some(())?;
    (i[16].b == i[11].a && i[16].c == i[15].a && i[17].a == total && i[17].b == i[16].a).then_some(())?;
    (i[21].b == i[19].a && i[21].c == i[20].a && i[22].a == index && i[22].b == i[21].a).then_some(())?;
    (i[19].b == index && i[25].b == total && i[26].a == i[25].a).then_some(())
}

fn require_object(code: CodeView<'_>, pc: usize, source: u16) -> Option<()> {
    matches!(code.cold_at(pc), Some(crate::ops::Op::RequireObjectCoercible { src }) if *src == source).then_some(())
}
