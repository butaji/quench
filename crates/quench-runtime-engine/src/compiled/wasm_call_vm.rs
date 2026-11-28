//! The VM callout of a stitched caller's `CALL_VM` row (`quench-bjit/stencils/wasm_rows_call_vm.rs`):
//! a direct call of a defined function the stitcher did not stitch. The callee runs in the VM, as
//! `reference_callout_inner` runs a target: its compiled entry when the program has one
//! (`run_compiled`), else its function value through `call_value`.
//!
//! The operand is `CallKnown`'s defined-function index. The VM resolves functions by Wasm's
//! absolute index (imports first), so the callout converts it as `callout_known_fallback` does.
//! The argument words are the callee's parameters and the result word goes to `out`. A trap or an
//! error leaves through `finish_callout`, as the other callouts do. The memory's length is
//! refreshed (`refresh_memory_length`), since the callee may have grown the memory.
//!
//! A child of `compiled` (`mod wasm_call_vm;`), so it names `compiled`'s helpers through `super`.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// `vm_call_callout`, the `CALL_VM` row's callout: (context, defined function index,
/// argument words, result word) -> status, 0 when the call returned.
pub(crate) unsafe extern "C" fn vm_call_callout(
    context: *mut CallContext,
    function: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: the row passes the context its entry was called with, whose VM is suspended in
    // that call, and the argument and result words of this call.
    let context = unsafe { &mut *context };
    note_compiled_callout(context.known_callouts);
    let vm_pointer = context.vm.cast::<Vm>();
    let (program, env) = (context.program, context.env);
    // The VM's own code runs outside compiled code: no guard fault is ours.
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: the VM is the context's, suspended in the entry call (see above).
        let vm = unsafe { &mut *vm_pointer };
        unsafe { run_defined(vm, program, env, function, args, out) }
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));
    // SAFETY: as above; the callee may have grown the memory.
    refresh_memory_length(context, unsafe { &*vm_pointer });
    finish_callout(context, called)
}

/// Runs defined function `function` of `program` in `env` with the argument words at `args`, its
/// result word to `out`.
unsafe fn run_defined(
    vm: &mut Vm,
    program: ProgramId,
    env: Value,
    function: u64,
    args: *const u64,
    out: *mut u64,
) -> Result<(), JsError> {
    let defined = u32::try_from(function).map_err(|_| JsError::validation("invalid Wasm function index".into()))?;
    let (params, result_type) = signature_of(vm, program, defined)?;
    // SAFETY: the stitched caller passes one word per parameter.
    let values = unsafe { encode_arguments(vm, &params, args)? };
    let result = match vm.run_compiled(program, env, defined, &values) {
        Some(result) => result?,
        None => call_in_vm(vm, program, env, defined, &values)?,
    };
    let bits = result_bits(vm, program, result, result_type)?;
    // SAFETY: `out` is the destination word the row names.
    unsafe { *out = bits };
    Ok(())
}

/// The parameter types and the result type of defined function `defined`.
fn signature_of(
    vm: &Vm,
    program: ProgramId,
    defined: u32,
) -> Result<(smallvec::SmallVec<[quench_runtime::WasmType; 8]>, Option<quench_runtime::WasmType>), JsError> {
    let signature = vm
        .programs
        .wasm_signatures(program)
        .and_then(|signatures| signatures.defined_signature(defined))
        .ok_or_else(|| JsError::validation("missing Wasm function signature".into()))?;
    Ok((signature.params.iter().copied().collect(), signature.results.first().copied()))
}

/// The argument words as Wasm values: an i32 from its low word, as the callout ABI carries it.
///
/// # Safety
/// `args` holds one word per parameter.
unsafe fn encode_arguments(
    vm: &mut Vm,
    params: &[quench_runtime::WasmType],
    args: *const u64,
) -> Result<smallvec::SmallVec<[Value; 8]>, JsError> {
    let mut values = smallvec::SmallVec::new();
    for (offset, ty) in params.iter().enumerate() {
        Kind::of(*ty).ok_or_else(|| JsError::validation("unsupported Wasm call parameter".into()))?;
        // SAFETY: the caller passes a word per parameter.
        let bits = unsafe { *args.add(offset) };
        values.push(wasm_word_value(vm, *ty, bits));
    }
    Ok(values)
}

/// The VM's call of defined function `defined` without a compiled entry: its function value,
/// resolved by Wasm's absolute index, called with the void receiver.
fn call_in_vm(
    vm: &mut Vm,
    program: ProgramId,
    env: Value,
    defined: u32,
    values: &[Value],
) -> Result<Value, JsError> {
    let absolute = absolute_index(vm, program, defined)?;
    let code = vm.programs.get(program).ok_or_else(|| JsError::validation("missing Wasm program".into()))?;
    let active_program = vm.replace_active_program(program);
    let callee = vm.wasm_function_reference(&code, absolute, env);
    vm.set_active_program(active_program);
    let callee = callee?;
    let receiver = vm
        .programs
        .constant(program, VOID_RESULT_CONSTANT)
        .ok_or_else(|| JsError::validation("missing Wasm void constant".into()))?;
    vm.call_value(&code, callee, receiver, values)
}

/// The absolute index of defined function `defined`: the imports come first.
fn absolute_index(vm: &Vm, program: ProgramId, defined: u32) -> Result<u32, JsError> {
    let imports = vm
        .programs
        .wasm_signatures(program)
        .ok_or_else(|| JsError::validation("missing Wasm function facts".into()))?
        .imports()
        .len();
    u32::try_from(imports)
        .ok()
        .and_then(|imports| imports.checked_add(defined))
        .ok_or_else(|| JsError::validation("invalid Wasm function index".into()))
}

/// The result word of `result` as `result_type` (none for a void function, whose word is 0): a
/// reference's Value bits, checked against its type under `program`'s declarations (a declared
/// type's identity is theirs), else the scalar's bits. Every callout that runs a Wasm call in the
/// VM returns its result word so.
pub(crate) fn result_bits(
    vm: &Vm,
    program: ProgramId,
    result: Value,
    result_type: Option<quench_runtime::WasmType>,
) -> Result<u64, JsError> {
    let Some(ty) = result_type else {
        return Ok(0);
    };
    if Kind::of(ty) == Some(Kind::Reference) {
        let declarations = vm.programs.wasm_signatures(program).map(|signatures| &signatures.declarations);
        vm.decode_wasm_value_in(result, ty, declarations)?;
        return Ok(result.to_bits());
    }
    match vm
        .decode_wasm_value(result, ty)?
        .bits()
        .ok_or_else(|| JsError::validation("non-scalar Wasm call result".into()))?
    {
        quench_runtime::wasm::ScalarBits::Bits32(bits) => Ok(u64::from(bits)),
        quench_runtime::wasm::ScalarBits::Bits64(bits) => Ok(bits),
        quench_runtime::wasm::ScalarBits::Bits128(_) => Err(JsError::validation("non-scalar Wasm call result".into())),
    }
}
