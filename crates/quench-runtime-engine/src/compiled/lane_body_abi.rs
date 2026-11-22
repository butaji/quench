//! The lane body ABI: a body returns its status and result word in two registers.

use super::*;

pub(crate) use quench_runtime::vm::engine_types::BodyReturn;

pub(crate) use quench_runtime::vm::engine_types::BodyEntry;

/// The lane body ABI: the shared native-entry ABI without the result pointer, whose word the
/// body returns beside its status (`BodyReturn`).
pub(crate) fn body_signature_in_lane(
    pointer: ir::Type,
    call_conv: cranelift_codegen::isa::CallConv,
) -> Signature {
    call_signature::typed_signature(call_conv, &[pointer, types::I64, types::I64, pointer], &[types::I32, types::I64])
}

/// Runs lane body `entry` through the fault-resuming trampoline: the shared trampoline
/// passes registers through, so a body's second result register reaches its caller.
///
/// # Safety
/// As `enter_wasm`, for a body of the lane body ABI.
pub(crate) unsafe fn enter_body(context: &mut CallContext, this: u64, captures: u64, registers: *const u64, entry: BodyEntry) -> BodyReturn {
    /// The trampoline typed for a body: its fifth argument is unused.
    type BodyTrampoline = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64, BodyEntry) -> BodyReturn;
    let pointer: *mut CallContext = context;
    // SAFETY: as the caller promises; one machine-code trampoline, and both ABIs return in the
    // same registers.
    unsafe { enter_compiled(pointer, |trampoline| match trampoline {
        Some(trampoline) => std::mem::transmute::<Trampoline, BodyTrampoline>(trampoline)(pointer, this, captures, registers, std::ptr::null_mut(), entry),
        None => entry(pointer, this, captures, registers),
    }) }
}
