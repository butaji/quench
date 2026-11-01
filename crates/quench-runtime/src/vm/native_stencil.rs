//! Copy-and-patch publication of the call-threaded native graph.
//!
//! The call-threaded graph (`native_graph::lower_body`) residualizes exactly
//! one static fact per instruction beyond dispatch: which semantic operation
//! runs at which PC with which resolved operands, and where control goes
//! next. Every instruction therefore has the same residual shape, differing
//! only in those static values. Running Cranelift over every function to
//! rediscover that shape spends compile time, temporary memory and, for large
//! functions, a region partition, on code it cannot improve: each semantic
//! operation is an opaque call.
//!
//! In Futamura's terms this module moves the second projection to process
//! start. Cranelift compiles each residual shape once, as a *stencil* whose
//! static inputs are left as relocations against named [`Hole`]s. Specializing
//! a program is then copying stencils and patching their holes
//! ([Copy-and-Patch](https://doi.org/10.1145/3485513)): no IR, no register
//! allocation and no per-function code mapping. Each instruction stencil is a
//! tail-calling continuation; a fallthrough step's final jump to its successor
//! is dropped when that successor is the next stencil in memory.
//!
//! Semantics are unchanged: the stencils call the same per-opcode semantic
//! entries and follow the same continuation protocol (status word, next PC in
//! the result word, exact-PC dispatch and invalid-PC exit) as the Cranelift
//! graph, so either publication can run any program.

use super::compiled::{
    CodeMemory, NATIVE_JS_INVALID_PC, NATIVE_JS_STOPPED, native_entry_signature,
    native_js_operation_signature, native_js_start_pc,
};
use super::specialize::{CallThreaded, CallThreadedExtension, Continuation, InstructionSite, Specializer};
use crate::Host;
use crate::bytecode::ResidualProgram;
use cranelift_codegen::{
    FinalizedRelocTarget,
    binemit::Reloc,
    ir::{self, InstBuilder, MemFlagsData, condcodes::IntCC, types},
    isa::{CallConv, TargetIsa},
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use std::{rc::Rc, sync::OnceLock};

/// Stencils are compiled in isolation, so every external name they declare is
/// one of their own holes; a single namespace suffices.
const HOLE_NAMESPACE: u32 = 0;

/// A static input of a stencil, supplied per copy. Holes are the stencil's
/// binding-time division made concrete: everything here is fixed by the
/// residual program, everything else is computed when the code runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum Hole {
    /// The opcode's monomorphized semantic entry (`Op::native_semantic_entry`).
    Operation,
    /// The instruction's PC.
    Pc,
    /// The address of the instruction's resolved operands.
    Operands,
    /// The fallthrough PC, one past `Pc`.
    NextPc,
    /// The function's instruction count, bounding exact-PC dispatch.
    Count,
    /// The function's PC-indexed table of instruction stencil addresses.
    Table,
    /// The fallthrough instruction's stencil: a continuation.
    Next,
    /// The function's dispatch stencil: a continuation.
    Dispatch,
}

impl Hole {
    const ALL: [Self; 8] = [
        Self::Operation,
        Self::Pc,
        Self::Operands,
        Self::NextPc,
        Self::Count,
        Self::Table,
        Self::Next,
        Self::Dispatch,
    ];

    fn from_index(index: u32) -> Option<Self> {
        Self::ALL.get(usize::try_from(index).ok()?).copied()
    }
}

/// One relocation of a stencil, resolved to the hole that fills it.
#[derive(Clone, Copy, Debug)]
struct Patch {
    offset: usize,
    kind: Reloc,
    hole: Hole,
    addend: i64,
}

/// Machine code with holes, compiled once per process.
struct Stencil {
    code: Box<[u8]>,
    patches: Box<[Patch]>,
    /// The length that remains when the final jump to [`Hole::Next`] is
    /// dropped because the successor stencil follows immediately.
    fallthrough_length: Option<usize>,
}

impl Stencil {
    fn copy_length(&self, elide_next: bool) -> usize {
        match (elide_next, self.fallthrough_length) {
            (true, Some(length)) => length,
            _ => self.code.len(),
        }
    }
}

/// The residual shapes of the call-threaded graph.
struct StencilLibrary {
    /// The shared entry ABI: reads the start PC and enters dispatch.
    entry: Stencil,
    /// Exact-PC dispatch through the function's table, or the invalid-PC exit.
    dispatch: Stencil,
    /// An instruction whose layout falls through: continue at `Next` when the
    /// operation reports the fallthrough PC, else dispatch.
    step: Stencil,
    /// An instruction that may transfer control anywhere: always dispatch.
    branch: Stencil,
    /// A transfer the specializer computed: continue at `Next`, no entry runs.
    jump: Stencil,
    /// Every stencil copy starts on this boundary.
    alignment: usize,
}

fn library() -> Option<&'static StencilLibrary> {
    static LIBRARY: OnceLock<Option<StencilLibrary>> = OnceLock::new();
    LIBRARY.get_or_init(StencilLibrary::compile).as_ref()
}

/// Whether this process can publish copy-and-patch code.
pub(super) fn available() -> bool {
    library().is_some()
}

#[derive(Clone, Copy)]
enum StepShape {
    Fallthrough,
    Branch,
}

impl StencilLibrary {
    fn step_for(&self, site: &InstructionSite) -> &Stencil {
        match site.continuation {
            Continuation::Fallthrough => &self.step,
            Continuation::Dispatch => &self.branch,
            Continuation::Static(_) => &self.jump,
        }
    }

    fn compile() -> Option<Self> {
        let isa = super::jit_backend::target_isa_for(
            "speed",
            super::jit_backend::CodePublication::Stencil,
        )?;
        if isa.pointer_type() != types::I64 {
            return None;
        }
        let alignment = usize::try_from(isa.function_alignment().minimum).ok()?.max(1);
        Some(Self {
            entry: Stencil::extract(entry_stencil(&*isa), &*isa)?,
            dispatch: Stencil::extract(dispatch_stencil(&*isa), &*isa)?,
            step: Stencil::extract(step_stencil(&*isa, StepShape::Fallthrough), &*isa)?,
            branch: Stencil::extract(step_stencil(&*isa, StepShape::Branch), &*isa)?,
            jump: Stencil::extract(jump_stencil(&*isa), &*isa)?,
            alignment,
        })
    }
}

impl Stencil {
    /// Compile one shape and keep its relocations as holes. Any relocation
    /// this module cannot patch rejects the whole library, which leaves
    /// publication to the Cranelift graph.
    fn extract(function: ir::Function, isa: &dyn TargetIsa) -> Option<Self> {
        super::jit_backend::compile_function(function, isa, None, |compiled, function| {
            let names = function.params.user_named_funcs();
            let mut patches = Vec::new();
            for relocation in compiled.buffer.relocs() {
                let FinalizedRelocTarget::ExternalName(ir::ExternalName::User(name)) =
                    relocation.target
                else {
                    return Err(super::jit_backend::NativeCompileRejection::Relocations);
                };
                let name = names
                    .get(name)
                    .filter(|name| name.namespace == HOLE_NAMESPACE)
                    .ok_or(super::jit_backend::NativeCompileRejection::Relocations)?;
                let hole = Hole::from_index(name.index)
                    .ok_or(super::jit_backend::NativeCompileRejection::Relocations)?;
                if !patchable(relocation.kind) {
                    return Err(super::jit_backend::NativeCompileRejection::Relocations);
                }
                patches.push(Patch {
                    offset: usize::try_from(relocation.offset)
                        .map_err(|_| super::jit_backend::NativeCompileRejection::Relocations)?,
                    kind: relocation.kind,
                    hole,
                    addend: relocation.addend,
                });
            }
            let code: Box<[u8]> = compiled.code_buffer().into();
            let fallthrough_length = trailing_next_jump(&code, &patches);
            Ok(Self { code, patches: patches.into_boxed_slice(), fallthrough_length })
        })
        .ok()
    }
}

fn patchable(kind: Reloc) -> bool {
    matches!(
        kind,
        Reloc::Abs8 | Reloc::X86PCRel4 | Reloc::X86CallPCRel4 | Reloc::X86CallPLTRel4 | Reloc::Arm64Call
    )
}

/// x86-64 `jmp rel32`: one opcode byte followed by the patched displacement.
const X86_JMP_REL32_OPCODE: u8 = 0xe9;
const X86_REL32_BYTES: usize = 4;
/// AArch64 `b imm26`: the six opcode bits above the 26-bit word offset.
const ARM64_BRANCH_IMMEDIATE_BITS: u32 = 26;
const ARM64_BRANCH_OPCODE: u32 = 0b000101;
const ARM64_INSTRUCTION_BYTES: usize = 4;
const ARM64_INSTRUCTION_ALIGNMENT_BITS: u32 = 2;

/// The stencil length without its final unconditional jump to `Next`, when
/// that jump is the last instruction. Copying the successor directly after the
/// shortened stencil turns the jump into a fallthrough.
fn trailing_next_jump(code: &[u8], patches: &[Patch]) -> Option<usize> {
    let next = patches.iter().find(|patch| patch.hole == Hole::Next)?;
    match next.kind {
        Reloc::X86CallPCRel4 | Reloc::X86PCRel4 | Reloc::X86CallPLTRel4 => {
            let opcode = next.offset.checked_sub(1)?;
            (next.offset + X86_REL32_BYTES == code.len() && code[opcode] == X86_JMP_REL32_OPCODE)
                .then_some(opcode)
        }
        Reloc::Arm64Call => {
            let word = code.get(next.offset..next.offset + ARM64_INSTRUCTION_BYTES)?;
            let word = u32::from_le_bytes(word.try_into().ok()?);
            (next.offset + ARM64_INSTRUCTION_BYTES == code.len()
                && word >> ARM64_BRANCH_IMMEDIATE_BITS == ARM64_BRANCH_OPCODE)
                .then_some(next.offset)
        }
        _ => None,
    }
}

/// Fill one hole of a copied stencil. `address` is the copy's final address.
fn patch(code: &mut [u8], address: u64, patch: &Patch, value: u64) -> Result<(), String> {
    let site = address
        .checked_add(patch.offset as u64)
        .ok_or("stencil patch address overflow")?;
    let relative = || -> Result<i64, String> {
        (value as i64)
            .checked_add(patch.addend)
            .and_then(|target| target.checked_sub(site as i64))
            .ok_or_else(|| "stencil displacement overflow".to_owned())
    };
    let bytes = code
        .get_mut(patch.offset..)
        .ok_or("stencil patch is outside its code")?;
    match patch.kind {
        Reloc::Abs8 => {
            let word = value.wrapping_add(patch.addend as u64).to_le_bytes();
            bytes.get_mut(..word.len()).ok_or("stencil patch is outside its code")?.copy_from_slice(&word);
        }
        Reloc::X86PCRel4 | Reloc::X86CallPCRel4 | Reloc::X86CallPLTRel4 => {
            let displacement = i32::try_from(relative()?)
                .map_err(|_| "stencil continuation is out of rel32 range")?
                .to_le_bytes();
            bytes.get_mut(..displacement.len()).ok_or("stencil patch is outside its code")?.copy_from_slice(&displacement);
        }
        Reloc::Arm64Call => {
            let delta = relative()?;
            let words = delta >> ARM64_INSTRUCTION_ALIGNMENT_BITS;
            let limit = 1_i64 << (ARM64_BRANCH_IMMEDIATE_BITS - 1);
            if delta & ((1 << ARM64_INSTRUCTION_ALIGNMENT_BITS) - 1) != 0 || !(-limit..limit).contains(&words) {
                return Err("stencil continuation is out of branch range".into());
            }
            let field = bytes.get_mut(..ARM64_INSTRUCTION_BYTES).ok_or("stencil patch is outside its code")?;
            let mask = (1_u32 << ARM64_BRANCH_IMMEDIATE_BITS) - 1;
            let word = u32::from_le_bytes((&*field).try_into().expect("four-byte instruction"));
            field.copy_from_slice(&((word & !mask) | (words as u32 & mask)).to_le_bytes());
        }
        _ => return Err("stencil relocation kind is not patchable".into()),
    }
    Ok(())
}

// -- Stencil shapes ---------------------------------------------------------

fn hole_value(builder: &mut FunctionBuilder<'_>, isa: &dyn TargetIsa, hole: Hole) -> ir::Value {
    let name = builder
        .func
        .declare_imported_user_function(ir::UserExternalName::new(HOLE_NAMESPACE, hole as u32));
    let symbol = builder.create_global_value(ir::GlobalValueData::Symbol {
        name: ir::ExternalName::user(name),
        offset: ir::immediates::Imm64::new(0),
        colocated: false,
        tls: false,
    });
    builder.ins().symbol_value(isa.pointer_type(), symbol)
}

fn continuation(builder: &mut FunctionBuilder<'_>, isa: &dyn TargetIsa, hole: Hole, with_pc: bool) -> ir::FuncRef {
    let signature = builder.import_signature(continuation_signature(isa, with_pc));
    let name = builder
        .func
        .declare_imported_user_function(ir::UserExternalName::new(HOLE_NAMESPACE, hole as u32));
    builder.import_function(ir::ExtFuncData {
        name: ir::ExternalName::user(name),
        signature,
        colocated: true,
        patchable: false,
    })
}

/// Continuations carry the context and result word between stencils in
/// registers; dispatch also receives the PC to select. Everything else a
/// stencil needs is static, so it is a hole rather than a live register.
fn continuation_signature(isa: &dyn TargetIsa, with_pc: bool) -> ir::Signature {
    let pointer = isa.pointer_type();
    let mut signature = ir::Signature::new(CallConv::Tail);
    for parameter in [pointer, pointer] {
        signature.params.push(ir::AbiParam::new(parameter));
    }
    if with_pc {
        signature.params.push(ir::AbiParam::new(types::I64));
    }
    signature.returns.push(ir::AbiParam::new(types::I32));
    signature
}

fn stencil_function(
    isa: &dyn TargetIsa,
    signature: ir::Signature,
    body: impl FnOnce(&mut FunctionBuilder<'_>, &[ir::Value]),
) -> ir::Function {
    let mut function = ir::Function::with_name_signature(ir::UserFuncName::default(), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let parameters = builder.block_params(entry).to_vec();
    body(&mut builder, &parameters);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    function
}

/// The shared native entry ABI (context, base, length, arguments, result).
fn entry_stencil(isa: &dyn TargetIsa) -> ir::Function {
    stencil_function(isa, native_entry_signature(isa.pointer_type(), isa.default_call_conv()), |builder, parameters| {
        let (context, result) = (parameters[0], parameters[4]);
        let start = native_js_start_pc(builder, context);
        let dispatch = continuation(builder, isa, Hole::Dispatch, true);
        let call = builder.ins().call(dispatch, &[context, result, start]);
        let status = builder.inst_results(call)[0];
        builder.ins().return_(&[status]);
    })
}

fn dispatch_stencil(isa: &dyn TargetIsa) -> ir::Function {
    stencil_function(isa, continuation_signature(isa, true), |builder, parameters| {
        let [context, result, pc] = parameters else { unreachable!("dispatch signature") };
        let count = hole_value(builder, isa, Hole::Count);
        let selected = builder.create_block();
        let invalid = builder.create_block();
        builder.set_cold_block(invalid);
        // Compare the full PC: a high word must not alias a valid instruction.
        let in_range = builder.ins().icmp(IntCC::UnsignedLessThan, *pc, count);
        builder.ins().brif(in_range, selected, &[], invalid, &[]);

        builder.switch_to_block(selected);
        let offset = builder.ins().imul_imm_u(*pc, i64::from(isa.pointer_bytes()));
        let table = hole_value(builder, isa, Hole::Table);
        let slot = builder.ins().iadd(table, offset);
        let target = builder.ins().load(isa.pointer_type(), MemFlagsData::trusted(), slot, 0);
        let signature = builder.import_signature(continuation_signature(isa, false));
        builder.ins().return_call_indirect(signature, target, &[*context, *result]);

        builder.switch_to_block(invalid);
        builder.ins().store(MemFlagsData::trusted(), *pc, *result, 0);
        let status = builder.ins().iconst(types::I32, i64::from(NATIVE_JS_INVALID_PC));
        builder.ins().return_(&[status]);
    })
}

fn jump_stencil(isa: &dyn TargetIsa) -> ir::Function {
    stencil_function(isa, continuation_signature(isa, false), |builder, parameters| {
        let following = continuation(builder, isa, Hole::Next, false);
        builder.ins().return_call(following, parameters);
    })
}

fn step_stencil(isa: &dyn TargetIsa, shape: StepShape) -> ir::Function {
    stencil_function(isa, continuation_signature(isa, false), |builder, parameters| {
        let [context, result] = parameters else { unreachable!("step signature") };
        let operation = hole_value(builder, isa, Hole::Operation);
        let pc = hole_value(builder, isa, Hole::Pc);
        let operands = hole_value(builder, isa, Hole::Operands);
        let signature = builder.import_signature(native_js_operation_signature(isa));
        let call = builder.ins().call_indirect(signature, operation, &[*context, pc, operands, *result]);
        let status = builder.inst_results(call)[0];

        // Blocks enter the layout in the order they are first switched to.
        // The fallthrough continuation comes last, so its tail jump is the
        // stencil's final instruction and can be elided.
        let stopped = builder.create_block();
        let continued = builder.create_block();
        let continued_test = builder.ins().icmp_imm_u(IntCC::Equal, status, 0);
        builder.ins().brif(continued_test, continued, &[], stopped, &[]);

        builder.switch_to_block(stopped);
        let stop = builder.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED));
        builder.ins().return_(&[stop]);

        builder.switch_to_block(continued);
        let next = builder.ins().load(types::I64, MemFlagsData::trusted(), *result, 0);
        let dispatch = continuation(builder, isa, Hole::Dispatch, true);
        match shape {
            StepShape::Branch => {
                builder.ins().return_call(dispatch, &[*context, *result, next]);
            }
            StepShape::Fallthrough => {
                let dispatched = builder.create_block();
                let fallthrough = builder.create_block();
                let expected = hole_value(builder, isa, Hole::NextPc);
                let falls = builder.ins().icmp(IntCC::Equal, next, expected);
                builder.ins().brif(falls, fallthrough, &[], dispatched, &[]);
                builder.switch_to_block(dispatched);
                builder.ins().return_call(dispatch, &[*context, *result, next]);
                builder.switch_to_block(fallthrough);
                let following = continuation(builder, isa, Hole::Next, false);
                builder.ins().return_call(following, &[*context, *result]);
            }
        }
    })
}

// -- Specialization ----------------------------------------------------------

/// Where one function's stencil copies live in the program's code image.
struct FunctionLayout {
    entry: usize,
    dispatch: usize,
    steps: Vec<StepCopy>,
}

/// One instruction's stencil copy: its offset, and whether its jump to the
/// following instruction was elided.
struct StepCopy {
    offset: usize,
    elide: bool,
}

/// The code, operand records and dispatch tables of one copy-and-patch
/// program. Executable code holds the record and table addresses, so all
/// three share the program's lifetime.
struct StencilImage {
    _code: CodeMemory,
    _sites: Vec<Box<[InstructionSite]>>,
    _tables: Vec<Box<[u64]>>,
}

/// Size and code bytes of a published copy-and-patch program, for measurement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeStencilStats {
    pub code_bytes: usize,
    pub instructions: usize,
    pub elided_jumps: usize,
    /// Instructions whose successor the specializer computed, so no
    /// semantic entry runs for them.
    pub static_transfers: usize,
}

/// The PC a stencil's `Next` hole continues at, when it has one.
fn successor(site: &InstructionSite) -> Option<usize> {
    match site.continuation {
        Continuation::Fallthrough => Some(site.pc + 1),
        Continuation::Static(target) => Some(target),
        Continuation::Dispatch => None,
    }
}

fn align_up(offset: usize, alignment: usize) -> Option<usize> {
    offset.checked_next_multiple_of(alignment)
}

/// The call-threaded generating extension that copies stencils. Each
/// instruction site selects a stencil by its continuation; its folded inputs
/// become hole values. Laying out a function only reserves space, so the
/// whole program is copied and patched into one mapping at `finish`.
struct StencilExtension<H> {
    library: &'static StencilLibrary,
    cursor: usize,
    layouts: Vec<FunctionLayout>,
    sites: Vec<Box<[InstructionSite]>>,
    stats: NativeStencilStats,
    host: std::marker::PhantomData<fn(&mut H)>,
}

impl<H> StencilExtension<H> {
    fn place(&mut self, stencil: &Stencil, elide: bool) -> Result<usize, String> {
        let start = align_up(self.cursor, self.library.alignment).ok_or("stencil image size overflow")?;
        self.cursor = start.checked_add(stencil.copy_length(elide)).ok_or("stencil image size overflow")?;
        Ok(start)
    }

    fn step(&self, site: &InstructionSite) -> &'static Stencil {
        self.library.step_for(site)
    }
}

impl<H: Host> CallThreadedExtension for StencilExtension<H> {
    type Residual = (StencilImage, Vec<crate::NativeJsFunctionEntry>, NativeStencilStats);

    fn function(&mut self, _function_id: u32, sites: Vec<InstructionSite>) -> Result<(), String> {
        let library = self.library;
        let entry = self.place(&library.entry, false)?;
        let dispatch = self.place(&library.dispatch, false)?;
        let mut steps = Vec::with_capacity(sites.len());
        for site in &sites {
            let stencil = self.step(site);
            // Elide only when the successor's aligned start is exactly here.
            let elide = successor(site) == Some(site.pc + 1)
                && stencil.fallthrough_length.is_some_and(|length| {
                    align_up(self.cursor, library.alignment)
                        .and_then(|start| start.checked_add(length))
                        .is_some_and(|end| align_up(end, library.alignment) == Some(end))
                });
            let offset = self.place(stencil, elide)?;
            steps.push(StepCopy { offset, elide });
            self.stats.elided_jumps += usize::from(elide);
            self.stats.static_transfers += usize::from(!site.calls_semantic_entry());
        }
        self.stats.instructions += sites.len();
        self.layouts.push(FunctionLayout { entry, dispatch, steps });
        self.sites.push(sites.into_boxed_slice());
        Ok(())
    }

    fn finish(self) -> Result<Self::Residual, String> {
        let Self { library, cursor, layouts, sites, mut stats, .. } = self;
        stats.code_bytes = cursor;
        let mut tables: Vec<Box<[u64]>> = layouts
            .iter()
            .map(|layout| vec![0_u64; layout.steps.len()].into_boxed_slice())
            .collect();
        let code = CodeMemory::assemble(cursor, |base, image| {
            let base = base as u64;
            let copy = |image: &mut [u8], stencil: &Stencil, offset: usize, elide: bool, value: &dyn Fn(Hole) -> Option<u64>| -> Result<(), String> {
                let length = stencil.copy_length(elide);
                let target = image.get_mut(offset..offset + length).ok_or("stencil copy is outside its image")?;
                target.copy_from_slice(&stencil.code[..length]);
                for hole in stencil.patches.iter().filter(|patch| patch.offset < length) {
                    let filled = value(hole.hole).ok_or("stencil hole has no value at this site")?;
                    patch(target, base + offset as u64, hole, filled)?;
                }
                Ok(())
            };
            for ((layout, sites), table) in layouts.iter().zip(&sites).zip(&tables) {
                let dispatch = base + layout.dispatch as u64;
                let table = table.as_ptr() as u64;
                let count = layout.steps.len() as u64;
                copy(image, &library.entry, layout.entry, false, &|hole| match hole {
                    Hole::Dispatch => Some(dispatch),
                    _ => None,
                })?;
                copy(image, &library.dispatch, layout.dispatch, false, &|hole| match hole {
                    Hole::Count => Some(count),
                    Hole::Table => Some(table),
                    _ => None,
                })?;
                for (site, step) in sites.iter().zip(&layout.steps) {
                    let next = successor(site)
                        .and_then(|pc| layout.steps.get(pc))
                        .map(|next| base + next.offset as u64);
                    let stencil = library.step_for(site);
                    let operation = site.op.native_semantic_entry::<H>() as usize as u64;
                    let operands = site.operands.as_ptr() as u64;
                    copy(image, stencil, step.offset, step.elide, &|hole| match hole {
                        Hole::Operation => Some(operation),
                        Hole::Pc => Some(site.pc as u64),
                        Hole::Operands => Some(operands),
                        Hole::NextPc => Some(site.pc as u64 + 1),
                        Hole::Next => next,
                        Hole::Dispatch => Some(dispatch),
                        Hole::Count | Hole::Table => None,
                    })?;
                }
            }
            Ok(())
        })?;
        let base = code.pointer() as usize;
        let mut entries = Vec::with_capacity(layouts.len());
        for (layout, table) in layouts.iter().zip(&mut tables) {
            for (slot, step) in table.iter_mut().zip(&layout.steps) {
                *slot = (base + step.offset) as u64;
            }
            // SAFETY: the entry stencil implements the shared native entry ABI,
            // and its copy for this function was patched with this function's table.
            entries.push(unsafe { std::mem::transmute::<usize, crate::NativeJsFunctionEntry>(base + layout.entry) });
        }
        Ok((StencilImage { _code: code, _sites: sites, _tables: tables }, entries, stats))
    }
}

/// Specialize every function of `program` by copying and patching stencils.
pub(super) fn compile_program<H: Host>(
    program: ResidualProgram,
) -> Result<(crate::NativeJsProgram<H>, NativeStencilStats), String> {
    let library = library().ok_or("copy-and-patch stencils are unavailable on this target")?;
    program.freeze_native_diagnostics();
    let extension = StencilExtension::<H> {
        library,
        cursor: 0,
        layouts: Vec::with_capacity(program.function_count()),
        sites: Vec::with_capacity(program.function_count()),
        stats: NativeStencilStats::default(),
        host: std::marker::PhantomData,
    };
    let (image, entries, stats) = CallThreaded(extension).specialize(&program)?;
    // SAFETY: each entry was specialized from this program's function for host
    // H, and the image keeps code, operand records and dispatch tables alive.
    let native = unsafe { crate::NativeJsProgram::from_entries(program, entries, Rc::new(image)) }?;
    Ok((native, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_host::TestHost;
    use crate::{Engine, ExecutionMode, Runtime, Value};

    const PROGRAM: &str = r#"
        globalThis.answer = 0;
        function grow(n) {
            let sum = 0;
            for (let i = 0; i < n; i++) {
                try { if (i === 3) throw i; sum += i; }
                catch (error) { sum += error * 2; }
                finally { sum += 1; }
            }
            return () => sum;
        }
        function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
        const shapes = [];
        for (let i = 0; i < 64; i++) shapes.push({ x: i, y: i * 2 });
        let total = 0;
        for (const shape of shapes) total += shape.x + shape.y;
        globalThis.saved = grow(8);
        answer = saved() + fib(15) + total;
        Promise.resolve().then(() => { answer += 1; });
    "#;

    /// Native recursion runs on the Rust stack; use the runtime's worker size
    /// rather than the test harness default.
    fn on_worker_stack(test: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(test)
            .unwrap()
            .join()
            .unwrap();
    }

    fn answer(runtime: &mut Runtime<TestHost>) -> Option<f64> {
        let global = runtime.global_root().unwrap();
        let key = runtime.string_rooted("answer");
        let answer = runtime.get_property_rooted(global, key).unwrap();
        runtime.rooted_value(answer).unwrap().as_number()
    }

    fn run_native(native: &crate::NativeJsProgram<TestHost>) -> Option<f64> {
        let mut runtime = Runtime::new(TestHost::default());
        runtime.set_execution_mode(ExecutionMode::NativeOnly);
        runtime.execute_native(native).unwrap();
        assert_eq!(runtime.native_execution_rejections(), 0);
        answer(&mut runtime)
    }

    #[test]
    fn stencil_library_resolves_every_hole_and_elides_fallthrough_jumps() {
        let library = library().expect("host target has a stencil library");
        for (stencil, holes) in [
            (&library.entry, &[Hole::Dispatch][..]),
            (&library.dispatch, &[Hole::Count, Hole::Table][..]),
            (&library.step, &[Hole::Operation, Hole::Pc, Hole::Operands, Hole::NextPc, Hole::Next, Hole::Dispatch][..]),
            (&library.branch, &[Hole::Operation, Hole::Pc, Hole::Operands, Hole::Dispatch][..]),
            (&library.jump, &[Hole::Next][..]),
        ] {
            for hole in holes {
                assert!(stencil.patches.iter().any(|patch| patch.hole == *hole), "missing {hole:?}");
            }
            assert!(stencil.patches.iter().all(|patch| holes.contains(&patch.hole)));
        }
        assert!(library.branch.fallthrough_length.is_none());
        if cfg!(target_arch = "x86_64") {
            let length = library.step.fallthrough_length.expect("x86-64 step ends in its Next jump");
            assert_eq!(length + 1 + X86_REL32_BYTES, library.step.code.len());
        }
    }

    #[test]
    fn stencil_publication_matches_cranelift_graph_and_interpreter() {
        on_worker_stack(|| {
            let program = Engine::specialize_unspecialized(PROGRAM, "stencil-parity.js").unwrap();
            let mut interpreted = Runtime::new(TestHost::default());
            interpreted.set_execution_mode(ExecutionMode::Interpreted);
            interpreted.execute(&program).unwrap();
            let expected = answer(&mut interpreted);
            assert!(expected.is_some());

            let (stencils, stats) = compile_program::<TestHost>(program.clone()).unwrap();
            assert_eq!(stats.instructions, (0..program.function_count()).map(|id| program.function(id).code.len()).sum::<usize>());
            if cfg!(target_arch = "x86_64") {
                assert!(stats.elided_jumps > 0);
            }
            let options = crate::NativeJsCodegenOptions { backend: crate::NativeJsBackend::Cranelift, ..Default::default() };
            let cranelift = crate::NativeJsProgram::compile_eager_with_options(program.clone(), options).unwrap();
            assert_eq!(run_native(&stencils), expected);
            assert_eq!(run_native(&cranelift), expected);
        });
    }

    #[test]
    fn stencil_closures_outlive_the_program_owner() {
        on_worker_stack(|| {
            let program = Engine::specialize_unspecialized(PROGRAM, "stencil-owner.js").unwrap();
            let (native, _) = compile_program::<TestHost>(program.clone()).unwrap();
            let mut runtime = Runtime::new(TestHost::default());
            runtime.set_execution_mode(ExecutionMode::NativeOnly);
            runtime.execute_native(&native).unwrap();
            drop(native);
            runtime.collect(&program).unwrap();
            let global = runtime.global_root().unwrap();
            let key = runtime.string_rooted("saved");
            let saved = runtime.get_property_rooted(global, key).unwrap();
            let receiver = runtime.root(Value::UNDEFINED);
            let value = runtime.call_rooted(saved, receiver, &[]).unwrap();
            assert_eq!(runtime.rooted_value(value).unwrap().as_number(), Some(39.0));
        });
    }

    #[test]
    fn empty_and_single_instruction_functions_dispatch_or_exit() {
        on_worker_stack(|| {
            for source in ["", "1;", "function f() {} f();"] {
                let program = Engine::specialize_unspecialized(source, "stencil-small.js").unwrap();
                let (native, _) = compile_program::<TestHost>(program).unwrap();
                let mut runtime = Runtime::new(TestHost::default());
                runtime.set_execution_mode(ExecutionMode::NativeOnly);
                runtime.execute_native(&native).unwrap();
            }
        });
    }
}
