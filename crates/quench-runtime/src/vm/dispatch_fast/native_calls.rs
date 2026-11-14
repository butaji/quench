//! Calls the lane answers in place for their common case (else `lane_native_call`), and
//! for-in's step (`lane_for_in_next`).
use super::*;
use crate::vm::object_get::ordinary_chain::ChainRead;

/// `charCodeAt` of a flat string at an integer index (`staged::builtins::char_code_at`): the
/// unit there, or NaN outside the string. `None` for any other receiver or index, which the
/// native coerces, and for a rope, which it flattens.
#[inline(always)]
pub(super) fn string_char_code_at<H: Host>(vm: &Vm<H>, receiver: Value, index: Value) -> Option<Value> {
    let records = vm.heap.string_record_base().unwrap_or(std::ptr::null());
    crate::staged::strings::concrete_string_char_code_at(records, receiver, index).or_else(|| crate::staged::builtins::concrete_char_code_at(vm.heap.string(receiver)?.existing_flat()?.view(), index))
}

/// `charAt` of a flat string at an integer index of an ASCII unit
/// (`staged::builtins::char_at_ascii`): the unit's shared string (`Vm::ascii_byte_string`).
/// Any other receiver, index or unit takes the native.
pub(super) fn string_char_at_ascii<H: Host>(vm: &mut Vm<H>, receiver: Value, index: Value) -> Option<Value> {
    let (records, table) = (vm.heap.string_record_base().unwrap_or(std::ptr::null()), vm.heap.ascii_unit_strings_base().unwrap_or(std::ptr::null()));
    if let Some(character) = crate::staged::strings::concrete_string_char_at(records, table, receiver, index) {
        return Some(character);
    }
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
    6 => WeakSetHas, 7 => StringCharAt, 8 => ArrayIsArray, 9 => StringCharCodeAt, 10 => ArrayPush, 11 => ArrayPop,
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
        native if let Some(row) = builtin_row(native) => builtin_handler::<H>(row),
        native if super::number::math_numbers_inline(native) => lane_call_math::<H>,
        native if native.is_array_callback() && result_dead() => lane_call_array_callback::<H, true>,
        native if native.is_array_callback() => lane_call_array_callback::<H, false>,
        _ => lane_native_call::<H>,
    }
}

/// A lane body's call of a native: one that completes in place (`Vm::in_place_native_value`: no
/// guest code, no raised error, no frame observed) writes its value to the call's register, which
/// the body reads as it reads a completed script call's, so the body stays native across
/// `Math.imul`, a string method or a collection read. Any other native is called out
/// (`lane_body_native_out`). A call that returns its result is a miss, which leaves the body at the call.
pub(super) fn lane_body_native_call<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, callee: Value) -> u64 {
    use super::super::compiled::LANE_CALL_COMPLETED;
    let (vm, i) = (unsafe { vm(cx) }, unsafe { &*ip });
    // A returning call's completion is its activation's, which the lane makes.
    if i.a & crate::bytecode::RETURN_REGISTER != 0 { return super::lane_body_stats::miss(super::lane_body_stats::Miss::NativeReturning); }
    let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = vm.heap.function(callee) else { return super::lane_body_stats::miss(super::lane_body_stats::Miss::NativeNotNative) };
    let native = *native;
    // SAFETY: the context's program outlives the lane run; the window is registers of the
    // running function, which the native reads before anything can move them.
    let (code, args) = unsafe { (&*(*cx).code, lane_call_window(r, i)) };
    let Some(value) = vm.in_place_native_value(code, native, callee, r.get(i.c), args) else {
        return lane_body_native_out(cx, r, ip, native, callee, args);
    };
    note_allocation(vm);
    r.set(i.a, value);
    LANE_CALL_COMPLETED
}

script_handler! {
    /// `lane_native_call` of `Reflect.get`, read once along an ordinary chain
    /// (`Vm::reflect_chain_read`): a data value or an absent key completes the call in place,
    /// an accessor with a getter is that getter's activation with the receiver argument as
    /// `this` (`lane_activate_getter`), whose completion is the call's value; any other
    /// call is the native's (`lane_native_call_value`).
    #[inline(never)]
    fn lane_reflect_get_call<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the window is registers of the running function, read before anything runs.
        let window = unsafe { lane_call_window(r, i) };
        if i.a & crate::bytecode::RETURN_REGISTER != 0 && !lane_returning_call_admitted(cx) {
            return exit(ip);
        }
        let read = match window {
            [target, key, ..] => vm.reflect_chain_read(*target, *key).map(|read| (read, window.get(2).copied().unwrap_or(*target))),
            _ => None,
        };
        match read {
            Some((ChainRead::Value(value), _)) => resume!(cx, lane_native_complete(cx, ip, i, Some(value))),
            Some((ChainRead::Absent, _)) => resume!(cx, lane_native_complete(cx, ip, i, Some(Value::UNDEFINED))),
            Some((ChainRead::Getter(Some(getter)), receiver)) if !getter.is_undefined() => {
                match lane_activate_getter(cx, r, ip, getter, receiver) {
                    Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
                    Err(next) => next,
                }
            }
            _ => resume!(cx, lane_native_call_value(cx, r, ip, i, Native::ReflectGet)),
        }
    }
}

/// The handler of a `Reflect.get` call site (`lane_native_call`'s sibling).
pub(super) fn reflect_get_call_handler<H: Host>() -> Handler {
    lane_reflect_get_call::<H>
}

/// `lane_body_native_call` of a native that is not in place: the lane's own call of it
/// (`lane_call_native`), beside the body's activations where the native allows, else from the
/// frame, so the callbacks it makes run as activations of the lane above the body
/// (`Vm::lane_deferred_callback`, `Vm::lane_native_callback`) and their returns go back to the
/// native, which returns to the body. `LANE_CALL_COMPLETED` with the value in the call's
/// register; `LANE_CALL_LEFT` where the native made the body's activations frames, whose
/// registers then hold it and which continue at the next record; `LANE_CALL_MISSED` where the
/// native is not one the lane calls or raised (the error stashed in `Vm::lane_raised` for the
/// general path, as `lane_body_value_step`'s).
fn lane_body_native_out<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, native: Native, callee: Value, args: &[Value]) -> u64 {
    use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_LEFT};
    let i = unsafe { &*ip };
    // An array callback method's callbacks are activations of the lane (`lane_begin_array_iteration`).
    if native.is_array_callback() && let Some(status) = lane_body_array_iteration(cx, ip, native, r.get(i.c), args) {
        return status;
    }
    let Some(value) = lane_call_native(cx, ip, native, callee, r.get(i.c), args) else {
        return super::lane_body_stats::miss(super::lane_body_stats::Miss::NativeNotCompleted);
    };
    // Guest code the native ran may have made the activations frames, whose registers these are.
    let after = unsafe { (*cx).registers };
    after.set(i.a, value);
    if after.0 == r.0 { return LANE_CALL_COMPLETED; }
    unsafe { (*cx).callee_resume = following(ip); (*cx).callee_acc = Value::UNDEFINED; }
    LANE_CALL_LEFT
}
