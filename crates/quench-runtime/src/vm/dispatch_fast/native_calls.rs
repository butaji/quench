//! Calls the lane answers in place for their common case (else `lane_native_call`), and
//! for-in's step (`lane_for_in_next`).
use super::*;

/// `charCodeAt` of a flat string at an integer index: the unit there, or NaN outside the
/// string. `None` for any other receiver or index, which the native coerces, and for a
/// rope, which it flattens.
#[inline(always)]
pub(super) fn string_char_code_at<H: Host>(vm: &Vm<H>, receiver: Value, index: Value) -> Option<Value> {
    let (units, index) = (vm.heap.string(receiver)?.existing_flat()?, index.as_int()?);
    Some(
        usize::try_from(index)
            .ok()
            .and_then(|index| units.unit(index))
            .map_or(Value::NAN, |unit| Value::integer(i32::from(unit))),
    )
}

/// `charAt` of a flat string at an integer index of an ASCII unit: the unit's shared string
/// (`Vm::ascii_unit_string`). Any other receiver, index or unit takes the native.
pub(super) fn string_char_at_ascii<H: Host>(vm: &mut Vm<H>, receiver: Value, index: Value) -> Option<Value> {
    let index = usize::try_from(index.as_int()?).ok()?;
    let unit = vm.heap.string(receiver)?.existing_flat()?.unit(index)?;
    vm.ascii_unit_string(unit)
}

script_handler! {
    /// A `Call` site whose callee was `String.prototype.charCodeAt` (`lane_script_call_resolve`):
    /// that call of a string at an integer index in place (`string_char_code_at`). Any other
    /// callee, receiver or index, or a call returning its result, takes `lane_native_call`.
    fn lane_call_char_code_at<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let is_char_code_at = matches!(
            vm.heap.function(r.get(i.b)),
            Some(FunctionRecord { kind: FunctionKind::Native(Native::StringCharCodeAt), .. })
        );
        // SAFETY: the call's window is registers of the running function.
        let index = unsafe { lane_call_window(r, i) }.first().copied();
        let Some(value) = index
            .filter(|_| is_char_code_at && i.a & crate::bytecode::RETURN_REGISTER == 0)
            .and_then(|index| string_char_code_at(vm, r.get(i.c), index))
        else {
            sibling!(lane_native_call::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The handler a `Call` site whose callee resolved to `native` takes from then on
/// (`lane_script_call_resolve`): a native whose common case the lane makes in place has its
/// own handler, which tests the callee's identity once and hands any other call to
/// `lane_native_call`, the handler of every other native. A call returning its result
/// completes its caller, which only `lane_native_call` makes. An array callback method's site
/// whose result nothing reads (`result_dead`, asked only there) takes the form that may
/// discard it (`lane_call_array_callback`'s `DISCARDS`).
pub(super) fn native_call_handler<H: Host>(native: Native, i: &LaneInstruction, result_dead: impl FnOnce() -> bool) -> Handler {
    if i.a & crate::bytecode::RETURN_REGISTER != 0 {
        return lane_native_call::<H>;
    }
    match native {
        Native::StringCharCodeAt => lane_call_char_code_at::<H>,
        Native::ArrayPush => lane_call_array_push::<H>,
        Native::ArrayPop => lane_call_array_pop::<H>,
        native if super::number::math_numbers_inline(native) => lane_call_math::<H>,
        native if native.is_array_callback() && result_dead() => lane_call_array_callback::<H, true>,
        native if native.is_array_callback() => lane_call_array_callback::<H, false>,
        _ => lane_native_call::<H>,
    }
}
