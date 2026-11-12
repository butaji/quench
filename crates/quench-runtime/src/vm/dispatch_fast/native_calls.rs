//! Calls the lane answers in place for their common case (else `lane_native_call`), and
//! for-in's step (`lane_for_in_next`).
use super::*;

/// `charCodeAt` of a flat string at an integer index (`staged::builtins::char_code_at`): the
/// unit there, or NaN outside the string. `None` for any other receiver or index, which the
/// native coerces, and for a rope, which it flattens.
#[inline(always)]
pub(super) fn string_char_code_at<H: Host>(vm: &Vm<H>, receiver: Value, index: Value) -> Option<Value> {
    crate::staged::builtins::concrete_char_code_at(vm.heap.string(receiver)?.existing_flat()?.view(), index)
}

/// `charAt` of a flat string at an integer index of an ASCII unit
/// (`staged::builtins::char_at_ascii`): the unit's shared string (`Vm::ascii_byte_string`).
/// Any other receiver, index or unit takes the native.
pub(super) fn string_char_at_ascii<H: Host>(vm: &mut Vm<H>, receiver: Value, index: Value) -> Option<Value> {
    let byte = crate::staged::builtins::concrete_char_at_ascii(vm.heap.string(receiver)?.existing_flat()?.view(), index)?;
    Some(vm.ascii_byte_string(byte))
}

/// The natives whose steady case (`Vm::steady_native_value`) a `Call` site makes under its own
/// handler, one instantiation of `lane_call_builtin` per row: the native is a constant of the
/// handler, so its entry is a direct call and the site tests only the callee's identity.
macro_rules! builtin_rows {
    ($($row:literal => $native:ident),* $(,)?) => {
        const fn row_native(row: u8) -> Native {
            match row { $($row => Native::$native,)* _ => unreachable!() }
        }
        fn builtin_row(native: Native) -> Option<u8> {
            match native { $(Native::$native => Some($row),)* _ => None }
        }
        fn builtin_handler<H: Host>(row: u8) -> Handler {
            match row { $($row => lane_call_builtin::<H, $row>,)* _ => lane_native_call::<H> }
        }
    };
}
builtin_rows!(
    0 => MapGet, 1 => MapHas, 2 => MapSet, 3 => SetHas, 4 => WeakMapGet, 5 => WeakMapHas,
    6 => WeakSetHas, 7 => StringCharAt, 8 => ArrayIsArray,
);

script_handler! {
    /// A `Call` site whose callee was the native of builtin row `ROW` (`native_call_handler`): that
    /// call in its steady case (`Vm::steady_native_value`). Any other callee or case takes
    /// `lane_native_call`.
    fn lane_call_builtin<ROW: u8>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let native = const { row_native(ROW) };
        let value = if callee_is_native(vm, r.get(i.b), native) {
            // SAFETY: the call's window is registers of the running function.
            vm.steady_native_value(native, r.get(i.c), unsafe { lane_call_window(r, i) })
        } else {
            None
        };
        let Some(value) = value else {
            sibling!(lane_native_call::<H>, r, ip, Carried::script(cx))
        };
        // A steady case may allocate; a due collection runs at the lane's next safepoint.
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
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
        native if let Some(row) = builtin_row(native) => builtin_handler::<H>(row),
        native if super::number::math_numbers_inline(native) => lane_call_math::<H>,
        native if native.is_array_callback() && result_dead() => lane_call_array_callback::<H, true>,
        native if native.is_array_callback() => lane_call_array_callback::<H, false>,
        _ => lane_native_call::<H>,
    }
}

/// A lane body's call of a native that completes in place (`Vm::in_place_native_value`: no
/// guest code, no raised error, no frame observed), its value in the call's register, which
/// the body reads as it reads a completed script call's: the body stays native across
/// `Math.imul`, a string method or a collection read instead of leaving at each. Any other
/// callee or case is a miss, which leaves the body at the call.
pub(super) fn lane_body_native_call<H: Host>(cx: *mut LaneContext<H>, r: Registers, i: &LaneInstruction, callee: Value) -> u64 {
    use super::super::compiled::LANE_CALL_COMPLETED;
    let vm = unsafe { vm(cx) };
    // A returning call's completion is its activation's, which the lane makes.
    if i.a & crate::bytecode::RETURN_REGISTER != 0 { return super::lane_body_stats::miss(super::lane_body_stats::Miss::NativeReturning); }
    let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = vm.heap.function(callee) else { return super::lane_body_stats::miss(super::lane_body_stats::Miss::NativeNotNative) };
    let native = *native;
    // SAFETY: the context's program outlives the lane run; the window is registers of the
    // running function, which the native reads before anything can move them.
    let (code, args) = unsafe { (&*(*cx).code, lane_call_window(r, i)) };
    let Some(value) = vm.in_place_native_value(code, native, callee, r.get(i.c), args) else { return super::lane_body_stats::miss(super::lane_body_stats::Miss::NativeInPlaceDeclined) };
    note_allocation(vm);
    r.set(i.a, value);
    LANE_CALL_COMPLETED
}
