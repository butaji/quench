//! The lane body ABI: a body returns its status and result word in two registers.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

pub(crate) use quench_runtime::vm::engine_types::BodyReturn;

pub(crate) use quench_runtime::vm::engine_types::BodyEntry;

/// The namespace of the body shim's function name.
const BODY_SHIM_NAMESPACE: u32 = u32::from_be_bytes(*b"QBSH");

/// The namespace of the entry stub's function name.
const ENTRY_STUB_NAMESPACE: u32 = u32::from_be_bytes(*b"QBES");

/// The lane body ABI under `call_conv`: the shared native-entry ABI without the result pointer, whose word the
/// body returns beside its status (`BodyReturn`).
pub(crate) fn body_signature_under(pointer: ir::Type, call_conv: cranelift_codegen::isa::CallConv) -> Signature {
    call_signature::typed_signature(call_conv, &[pointer, types::I64, types::I64, pointer], &[types::I32, types::I64])
}

/// The lane body ABI of a body and its callers: the body convention (`BODY_CALL_CONV`), so a body may tail call another.
pub(crate) fn body_signature_in_lane(pointer: ir::Type) -> Signature {
    body_signature_under(pointer, BODY_CALL_CONV)
}

/// The convention of the body shim, which Rust and the trampoline call: the host's default, not the body's.
fn shim_call_conv() -> cranelift_codegen::isa::CallConv {
    cranelift_codegen::isa::CallConv::triple_default(&target_lexicon::Triple::host())
}

/// The signature of the body shim: a lane body's arguments, then the body it calls.
pub(crate) fn body_shim_signature(pointer: ir::Type) -> Signature {
    call_signature::typed_signature(shim_call_conv(), &[pointer, types::I64, types::I64, pointer, pointer], &[types::I32, types::I64])
}

/// The address of the body shim: one SysV function for every lane body, which calls the body it
/// receives as its fifth argument. Every native caller enters a body through it (`CallContext::body_shim`),
/// so a body's convention is the shim's alone. Zero where the host has no native code.
pub(crate) fn body_shim() -> usize {
    static SHIM: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *SHIM.get_or_init(|| published(build_body_shim))
}

/// The address of the entry stub: the entry of every unpublished cell (`EntryCell::stub`). It has the lane body
/// convention, as a body does, and calls `entry_stub_in_lane`, which enters nothing so its caller restarts the call.
/// Zero where the host has no native code.
pub(crate) fn entry_stub() -> usize {
    static STUB: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *STUB.get_or_init(|| published(build_entry_stub))
}

/// Compiles the function `build` makes for the host and publishes its code for the life of the process: the
/// memory is leaked, so the returned address stays callable. Zero where the host has no native code.
fn published(build: fn(&dyn cranelift_codegen::isa::TargetIsa) -> ir::Function) -> usize {
    let Some(isa) = quench_runtime::vm::jit_backend::target_isa() else { return 0 };
    let Some(memory) = super::compile_entry(&*isa, build(&*isa)).and_then(|code| CodeMemory::new(&code)) else { return 0 };
    Box::leak(Box::new(memory)).pointer().addr()
}

/// The entry stub's function: it takes a lane body's arguments and calls `entry_stub_in_lane` with them, which the
/// host's default convention takes, and returns its status and word.
fn build_entry_stub(isa: &dyn cranelift_codegen::isa::TargetIsa) -> ir::Function {
    let pointer = isa.pointer_type();
    let mut func = ir::Function::with_name_signature(ir::UserFuncName::user(ENTRY_STUB_NAMESPACE, 0), body_signature_in_lane(pointer));
    let mut builder_context = quench_runtime::vm::jit_backend::builder::take_context();
    let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
    let (block, parameters) = quench_runtime::vm::jit_backend::builder::enter(&mut builder);
    builder.seal_block(block);
    let target = builder.import_signature(body_signature_under(pointer, isa.default_call_conv()));
    let address = builder.ins().iconst(pointer, quench_runtime::vm::engine_types::entry_stub_in_lane as BodyEntry as usize as i64);
    let call = builder.ins().call_indirect(target, address, &parameters);
    let results = builder.inst_results(call).to_vec();
    builder.ins().return_(&results);
    builder.finalize(isa.frontend_config());
    quench_runtime::vm::jit_backend::builder::keep_context(builder_context);
    func
}

/// The body shim's function: it calls the body at its fifth parameter with the four lane body arguments,
/// and returns the body's status and word.
fn build_body_shim(isa: &dyn cranelift_codegen::isa::TargetIsa) -> ir::Function {
    let pointer = isa.pointer_type();
    let mut func = ir::Function::with_name_signature(ir::UserFuncName::user(BODY_SHIM_NAMESPACE, 0), body_shim_signature(pointer));
    let mut builder_context = quench_runtime::vm::jit_backend::builder::take_context();
    let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
    let (block, parameters) = quench_runtime::vm::jit_backend::builder::enter(&mut builder);
    builder.seal_block(block);
    let body = builder.import_signature(body_signature_in_lane(pointer));
    let call = builder.ins().call_indirect(body, parameters[4], &parameters[..4]);
    let results = builder.inst_results(call).to_vec();
    builder.ins().return_(&results);
    builder.finalize(isa.frontend_config());
    quench_runtime::vm::jit_backend::builder::keep_context(builder_context);
    func
}

/// The body shim's type: the context, receiver, capture word, registers, then the body it calls.
type BodyShimFn = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, usize) -> BodyReturn;

/// Calls lane body `entry` through the body shim of `context` (`CallContext::body_shim`), directly.
///
/// # Safety
/// As `enter_body`, for `entry` of the lane body ABI and the registers it names.
pub(crate) unsafe fn call_body_shim(context: *mut CallContext, this: u64, captures: u64, registers: *const u64, entry: BodyEntry) -> BodyReturn {
    // SAFETY: the context is live for the call, and its shim is the body shim's address (`body_shim`).
    let shim: BodyShimFn = unsafe { std::mem::transmute::<usize, BodyShimFn>((*context).head.body_shim) };
    // SAFETY: as the caller promises.
    unsafe { shim(context, this, captures, registers, entry as usize) }
}

/// Runs lane body `entry` through the body shim, which the fault-resuming trampoline calls: the
/// trampoline passes registers through, so a body's second result register reaches its caller.
///
/// # Safety
/// As `enter_wasm`, for a body of the lane body ABI.
pub(crate) unsafe fn enter_body(context: &mut CallContext, this: u64, captures: u64, registers: *const u64, entry: BodyEntry) -> BodyReturn {
    /// The trampoline typed for a body: its fifth argument is the body, its sixth the shim.
    type BodyTrampoline = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, usize, usize) -> BodyReturn;
    let pointer: *mut CallContext = context;
    let shim = context.head.body_shim;
    // SAFETY: as the caller promises; one machine-code trampoline, and both ABIs return in the
    // same registers.
    unsafe { enter_compiled(pointer, |trampoline| match trampoline {
        Some(trampoline) => std::mem::transmute::<Trampoline, BodyTrampoline>(trampoline)(pointer, this, captures, registers, entry as usize, shim),
        None => call_body_shim(pointer, this, captures, registers, entry),
    }) }
}
