//! The lane body ABI: a body returns its status and result word in two registers.

use super::*;

/// What a lane body returns in two registers: its status and its result word.
#[repr(C)]
pub(super) struct BodyReturn {
    pub(super) status: u32,
    pub(super) value: u64,
}

/// A lane body's entry: (context, receiver, capture word, registers) -> (status, result word).
pub(in crate::vm) type BodyEntry = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64) -> BodyReturn;

/// The words a body takes: context, receiver, capture word and registers.
pub(super) const LANE_BODY_PARAMS: usize = 4;

/// The lane body ABI: the shared native-entry ABI without the result pointer, whose word the
/// body returns beside its status (`BodyReturn`).
pub(super) fn lane_body_signature(
    pointer: ir::Type,
    call_conv: cranelift_codegen::isa::CallConv,
) -> Signature {
    let mut signature = Signature::new(call_conv);
    for parameter in [pointer, types::I64, types::I64, pointer] as [ir::Type; LANE_BODY_PARAMS] {
        signature.params.push(AbiParam::new(parameter));
    }
    signature.returns.extend([AbiParam::new(types::I32), AbiParam::new(types::I64)]);
    signature
}

/// Runs lane body `entry` through the fault-resuming trampoline: the shared trampoline
/// passes registers through, so a body's second result register reaches its caller.
///
/// # Safety
/// As `enter_native`, for a body of the lane body ABI.
pub(super) unsafe fn enter_body(context: &mut CallContext, this: u64, captures: u64, registers: *const u64, entry: BodyEntry) -> BodyReturn {
    /// The trampoline typed for a body: its fifth argument is unused.
    type BodyTrampoline = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64, BodyEntry) -> BodyReturn;
    match guard_trampoline() {
        Some(trampoline) => {
            let outer = ACTIVE_CONTEXT.with(|active| active.replace(context));
            // SAFETY: one machine-code trampoline; both ABIs return in the same registers.
            let trampoline = unsafe { std::mem::transmute::<Trampoline, BodyTrampoline>(trampoline) };
            // SAFETY: as the caller promises.
            let returned = unsafe { trampoline(context, this, captures, registers, std::ptr::null_mut(), entry) };
            ACTIVE_CONTEXT.with(|active| active.set(outer));
            returned
        }
        // SAFETY: as the caller promises.
        None => unsafe { entry(context, this, captures, registers) },
    }
}
