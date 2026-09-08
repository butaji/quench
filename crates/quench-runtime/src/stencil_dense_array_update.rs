//! Guarded dense-Number update loop with an ordered terminal reduction.
//!
//! The canonical residual CFG remains the semantic authority. This module
//! recognizes one reusable macro pattern, proves its operand relationships,
//! and reuses the published native numeric-array loop for the backedge. The
//! bounded tail reduction runs once after native completion, never per loop
//! operation or iteration.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView, NativeDispatchError};
use std::{cell::RefCell, rc::Rc};

pub(crate) const REGION_END: usize = 47;
const LOOP_HEADER: usize = 4;
const LOOP_BACKEDGE: usize = 26;
const LOOP_EXIT: usize = 27;
const REDUCTION_ELEMENTS: usize = 4;
const DENSE_UPDATE_REGION_ID: crate::stencil_fact::RegionId =
    crate::stencil_fact::RegionId(0x4455_5052);

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
    Opcode::Slow,
    Opcode::LoadLocal,
    Opcode::AGetI,
    Opcode::LoadLocal,
    Opcode::Add,
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
    Opcode::LoadConst,
    Opcode::AGetI,
    Opcode::Add,
    Opcode::LoadLocal,
    Opcode::Slow,
    Opcode::LoadConst,
    Opcode::AGetI,
    Opcode::Add,
    Opcode::LoadLocal,
    Opcode::Slow,
    Opcode::LoadConst,
    Opcode::AGetI,
    Opcode::Add,
    Opcode::Return,
];

#[derive(Clone, Copy)]
pub(crate) struct DenseUpdateSelection {
    array_slot: u16,
    delta_slot: u16,
    index_slot: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DenseUpdateOutcome {
    Completed(f64),
    Resume { pc: usize },
}

pub(crate) struct NativeDenseUpdatePlan {
    selection: DenseUpdateSelection,
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

impl NativeDenseUpdatePlan {
    pub(crate) fn new(
        selection: DenseUpdateSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.array_numeric_loops.then_some(())?;
        let view = crate::stencil_select::select_physical(
            crate::stencil_select::array_numeric_loop_region_key(),
        )?;
        (view.abi == crate::stencil_select::RegionAbi::ArrayNumericLoop
            && view.executable
            && view.stencil.validate())
        .then_some(())?;
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
    ) -> Result<Option<DenseUpdateOutcome>, NativeDispatchError> {
        if !environment.can_elide_terminal_store(self.selection.index_slot) {
            return Ok(None);
        }
        let Some(delta) = environment.get_number(self.selection.delta_slot) else {
            return Ok(None);
        };
        environment
            .with_proven_array(self.selection.array_slot, |array| {
                self.execute_array(array, delta, environment, context)
            })
            .unwrap_or(Ok(None))
    }

    fn execute_array(
        &mut self,
        array: &crate::value::ArrayData,
        delta: f64,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Result<Option<DenseUpdateOutcome>, NativeDispatchError> {
        if !array.is_plain_dense_access() || array.len() != REDUCTION_ELEMENTS {
            return Ok(None);
        }
        let mut words = array.numeric_kernel_words_mut().ok_or_else(|| {
            NativeDispatchError::Physical("dense update backing changed before entry".into())
        })?;
        let mut native = self.native_context(&mut words, delta, context);
        let status = self.invoke(&mut native)?;
        let outcome = finish_native(status, &mut native, &words)?;
        if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
            context.clear_interrupt();
        }
        drop(words);
        if let DenseUpdateOutcome::Resume { .. } = outcome {
            environment.set(
                self.selection.index_slot,
                crate::value::Value::Number(native.index as f64),
            );
        }
        Ok(Some(outcome))
    }

    pub(crate) fn route() -> impl Iterator<Item = &'static str> {
        [
            "LoadLocalChecked",
            "GetN",
            "Binary",
            "JumpIfFalse",
            "AGetI",
            "Add",
            "ASetI",
            "AddConst",
            "Jump",
            "Return",
        ]
        .into_iter()
    }

    fn native_context(
        &self,
        words: &mut [f64],
        delta: f64,
        context: &crate::vm::VmContext,
    ) -> crate::vm::NativeArrayLoopContext {
        crate::vm::NativeArrayLoopContext {
            data: words.as_mut_ptr(),
            len: words.len(),
            index: 0,
            end: words.len(),
            addend: delta,
            result: 0.0,
            interrupt: context.interrupt_flag(),
        }
    }

    fn invoke(
        &mut self,
        context: &mut crate::vm::NativeArrayLoopContext,
    ) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        let lease = crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry)
            .map_err(|error| {
                NativeDispatchError::Physical(format!("dense update lease: {error:?}"))
            })?;
        lease
            .invoke(|call| call((context as *mut crate::vm::NativeArrayLoopContext).cast()))
            .map_err(|error| {
                NativeDispatchError::Physical(format!("dense update invoke: {error:?}"))
            })
    }

    fn entry(
        &mut self,
    ) -> Result<
        crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>,
        NativeDispatchError,
    > {
        if let Some(entry) = self.installed {
            if self.owner.borrow().entry_token_is_live(entry) {
                return Ok(entry);
            }
            self.installed = None;
        }
        let address = self
            .owner
            .borrow_mut()
            .publish_region_image_or_get(&mut self.cache, &self.image)
            .map_err(|error| {
                NativeDispatchError::Physical(format!("dense update publish: {error:?}"))
            })?;
        let entry = self
            .owner
            .borrow()
            .owned_array_numeric_loop_entry(address)
            .map_err(|error| {
                NativeDispatchError::Physical(format!("dense update entry: {error:?}"))
            })?;
        self.installed = Some(entry);
        Ok(entry)
    }
}

fn finish_native(
    status: u64,
    native: &mut crate::vm::NativeArrayLoopContext,
    words: &[f64],
) -> Result<DenseUpdateOutcome, NativeDispatchError> {
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT && native.index < native.end {
        return Ok(DenseUpdateOutcome::Resume { pc: LOOP_HEADER });
    }
    let completed = status == crate::vm::NATIVE_DISPATCH_OK
        || (status == crate::vm::NATIVE_DISPATCH_INTERRUPT && native.index == native.end);
    if !completed || native.index != native.end {
        return Err(NativeDispatchError::committed(
            LOOP_BACKEDGE,
            "dense update native loop returned incomplete progress",
        ));
    }
    let mut result = *words.first().ok_or_else(|| {
        NativeDispatchError::committed(LOOP_EXIT, "dense update reduction is empty")
    })?;
    for value in &words[1..] {
        result += *value;
    }
    Ok(DenseUpdateOutcome::Completed(result))
}

fn region_image(
    view: crate::stencil_select::PhysicalStencilView,
) -> crate::stencil_region_layout::VerifiedRegionImage {
    let identity = crate::stencil_region_layout::RegionImageIdentity {
        key: crate::stencil_fact::RegionKey::from_opcodes(DENSE_UPDATE_REGION_ID, &OPERATIONS),
        cache_signature: byte_fingerprint(view.stencil.bytes),
        abi: crate::stencil_select::RegionAbi::ArrayNumericLoop,
    };
    crate::stencil_region_layout::VerifiedRegionImage::from_composed(
        identity,
        view.stencil.bytes.to_vec(),
    )
}

fn byte_fingerprint(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x1000_0000_01b3;
    bytes.iter().fold(FNV_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

pub(crate) fn select_dense_update(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<DenseUpdateSelection> {
    if start != 0 || entries.len() < REGION_END {
        return None;
    }
    cfg.region_control(start, REGION_END)?;
    let instructions = operation_window(entries)?;
    constants_match(code, &instructions)?;
    bindings_match(code, &instructions)?;
    Some(DenseUpdateSelection {
        array_slot: instructions[5].b,
        delta_slot: instructions[17].b,
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
        .all(|(instruction, opcode)| instruction.opcode == opcode)
        .then_some(instructions)
}

fn constants_match(code: CodeView<'_>, instructions: &[Instruction; REGION_END]) -> Option<()> {
    number_constant(code, instructions[1], 0.0)?;
    number_constant(code, instructions[22], 1.0)?;
    for (expected, pc) in [0.0, 1.0, 2.0, 3.0].into_iter().zip([29, 33, 38, 43]) {
        number_constant(code, instructions[pc], expected)?;
    }
    Some(())
}

fn number_constant(code: CodeView<'_>, instruction: Instruction, expected: f64) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    (value.to_bits() == expected.to_bits()).then_some(())
}

fn bindings_match(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    let index_slot = i[2].a;
    let array_slot = i[5].b;
    (i[2].b == i[1].a && i[4].b == index_slot && i[11].b == index_slot).then_some(())?;
    (i[15].b == index_slot && i[21].b == index_slot && i[24].a == index_slot).then_some(())?;
    (i[5].a == i[6].b && i[6].flags == crate::ir::GETN_LENGTH_FLAG).then_some(())?;
    (i[7].b == i[4].a && i[7].c == i[6].a && i[8].a == i[7].a).then_some(())?;
    (usize::from(i[8].b) == LOOP_EXIT && usize::from(i[26].a) == LOOP_HEADER).then_some(())?;
    body_bindings_match(code, i, array_slot)?;
    tail_bindings_match(code, i, array_slot)
}

fn body_bindings_match(
    code: CodeView<'_>,
    i: &[Instruction; REGION_END],
    array_slot: u16,
) -> Option<()> {
    (i[9].b == array_slot && i[13].b == array_slot).then_some(())?;
    (i[10].b == i[9].a && i[12].b == i[10].a).then_some(())?;
    require_object(code, 14, i[13].a)?;
    (i[16].b == i[13].a && i[16].c == i[15].a).then_some(())?;
    (i[18].b == i[16].a && i[18].c == i[17].a).then_some(())?;
    (i[19].a == i[12].a && i[19].b == i[11].a && i[19].c == i[18].a).then_some(())?;
    (i[23].b == i[21].a && i[23].c == i[22].a && i[24].b == i[23].a).then_some(())
}

fn tail_bindings_match(
    code: CodeView<'_>,
    i: &[Instruction; REGION_END],
    array_slot: u16,
) -> Option<()> {
    let chunks = [
        (27, 28, 29, 30),
        (31, 32, 33, 34),
        (36, 37, 38, 39),
        (41, 42, 43, 44),
    ];
    for (load, slow, constant, get) in chunks {
        (i[load].b == array_slot && i[get].b == i[load].a && i[get].c == i[constant].a)
            .then_some(())?;
        require_object(code, slow, i[load].a)?;
    }
    (i[35].b == i[30].a && i[35].c == i[34].a).then_some(())?;
    (i[40].b == i[35].a && i[40].c == i[39].a).then_some(())?;
    (i[45].b == i[40].a && i[45].c == i[44].a && i[46].a == i[45].a).then_some(())
}

fn require_object(code: CodeView<'_>, pc: usize, source: u16) -> Option<()> {
    matches!(
        code.cold_at(pc),
        Some(crate::ops::Op::RequireObjectCoercible { src }) if *src == source
    )
    .then_some(())
}
