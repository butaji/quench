//! A call the region runs natively: the callee's compiled parameters-only body runs through
//! `Vm::js_jit_compiled_call`, the arguments read from the window the lowering spilled, and
//! the result returns as a boxed value. Any miss (no compiled body, a non-Number argument, a
//! general-dispatch artifact) returns `None`, and the region leaves for the lane at the call.

use super::*;

/// The argument window's base and count fill the low two `u16` fields; the call's tag sits
/// above them, clear of the construct and field-store tags.
pub(super) const OSR_CALL: u64 = 1 << (3 * u16::BITS);
pub(super) const OSR_CALL_MASK: u64 = u64::MAX << (3 * u16::BITS);
/// The most arguments one call passes through the window.
const MAX_CALL_ARGUMENTS: usize = 16;

/// The word `word`'s field `shift` bits from its bottom, `bits` wide.
fn field(word: u64, shift: u32, bits: u32) -> usize {
    let mask = u64::MAX >> (u64::BITS - bits);
    ((word >> shift) & mask) as usize
}

/// A call from `osr_probe`: `receiver_bits` is the callee value, the window words are in the
/// activation file (an inlined callee's in the entry's scratch registers).
pub(super) fn call<H: Host>(
    vm: &mut Vm<H>,
    payload: &OsrProbePayload,
    program: &ResidualProgram,
    word: u64,
    receiver_bits: u64,
) -> Option<Value> {
    let base = field(word, u16::BITS, u16::BITS);
    let argc = field(word, 0, u16::BITS);
    if argc > MAX_CALL_ARGUMENTS {
        return None;
    }
    let callee = Value::from_bits(receiver_bits);
    let (id, function) = vm.compiled_call_target(callee)?;
    let caller_registers = unsafe { (*payload.region).inlining.as_ref() }
        .map_or(usize::MAX, |inlining| usize::from(inlining.caller_registers));
    let mut arguments = [Value::UNDEFINED; MAX_CALL_ARGUMENTS];
    for (index, argument) in arguments.iter_mut().enumerate().take(argc) {
        let register = base + index;
        // SAFETY: the lowering wrote the window to the file (or the entry's scratch)
        // immediately before this call.
        *argument = unsafe {
            if register < caller_registers {
                *payload.registers.add(register)
            } else {
                (*payload.entry).scratch_register(register - caller_registers)
            }
        };
    }
    vm.js_jit_compiled_call(program, (id, function), false, &arguments[..argc])
}

/// A dense element read from `osr_probe`: the key is in the scratch word.
pub(super) fn dense_read<H: Host>(vm: &mut Vm<H>, receiver_bits: u64, key_bits: u64) -> Option<Value> {
    vm.dense_array_element(Value::from_bits(receiver_bits), Value::from_bits(key_bits))
}

/// A frame-owned local read from `osr_probe`: the binding's value when the frame is the
/// region's own activation and the lane pushed no others.
pub(super) fn initialized_local<H: Host>(
    vm: &Vm<H>,
    payload: &OsrProbePayload,
    code: &ResidualProgram,
    receiver_bits: u64,
) -> Option<Value> {
    if vm.lane_has_activations()
        || vm.frames.get(payload.frame).is_none_or(|frame| frame.function != payload.function)
    {
        return None;
    }
    let slot = usize::try_from(receiver_bits).ok()?;
    vm.initialized_frame_binding(code, payload.frame, slot)
}

