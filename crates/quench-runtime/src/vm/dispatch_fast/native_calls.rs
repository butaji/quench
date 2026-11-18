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
    crate::staged::strings::concrete_string_char_code_at(records, receiver, index).or_else(|| crate::staged::builtins::concrete_char_code_at(vm.heap.flat_text(receiver)?.units(), index))
}

/// `charAt` of a flat string at an integer index of an ASCII unit
/// (`staged::builtins::char_at_ascii`): the unit's shared string (`Vm::ascii_byte_string`).
/// Any other receiver, index or unit takes the native.
pub(super) fn string_char_at_ascii<H: Host>(vm: &mut Vm<H>, receiver: Value, index: Value) -> Option<Value> {
    let (records, table) = (vm.heap.string_record_base().unwrap_or(std::ptr::null()), vm.heap.ascii_unit_strings_base().unwrap_or(std::ptr::null()));
    if let Some(character) = crate::staged::strings::concrete_string_char_at(records, table, receiver, index) {
        return Some(character);
    }
    let byte = crate::staged::builtins::concrete_char_at_ascii(vm.heap.flat_text(receiver)?.units(), index)?;
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
    12 => JsonStringify, 13 => DateNow, 14 => RegExpTest, 15 => RegExpExec,
);

/// The Math functions whose call a site makes under its own handler, one instantiation of
/// `lane_call_math_native` per row: the native is a constant, so the site tests the callee's
/// identity and runs the operation inline (`number::math_inline_call` folds to it) with no
/// dispatch on the native. Every other inline Math native keeps `lane_call_math`.
macro_rules! math_rows {
    ($($row:literal => $native:ident),* $(,)?) => {
        const fn math_row_native(row: u8) -> Native {
            match row { $($row => Native::$native,)* _ => unreachable!() }
        }
        fn math_handler<H: Host>(native: Native) -> Option<Handler> {
            match native { $(Native::$native => Some(lane_call_math_native::<H, $row>),)* _ => None }
        }
        fn math_field_call_handler<H: Host>(native: Native) -> Option<Handler> {
            match native { $(Native::$native => Some(lane_get_field_math_call::<H, $row>),)* _ => None }
        }
        fn math_field_call2_handler<H: Host>(native: Native, constant_second: bool) -> Option<Handler> {
            match (native, constant_second) {
                $((Native::$native, false) => Some(lane_get_field_math_call2::<H, $row, false>),
                  (Native::$native, true) => Some(lane_get_field_math_call2::<H, $row, true>),)*
                _ => None,
            }
        }
        pub(super) fn math_field_call_handlers<H: Host>() -> [*const (); 3 * [$($row),*].len()] {
            [$(lane_get_field_math_call::<H, $row> as *const (),
               lane_get_field_math_call2::<H, $row, false> as *const (),
               lane_get_field_math_call2::<H, $row, true> as *const ()),*]
        }
    };
}
math_rows!(
    0 => MathSqrt, 1 => MathAbs, 2 => MathFloor, 3 => MathCeil, 4 => MathRound, 5 => MathTrunc,
    6 => MathMin, 7 => MathMax, 8 => MathImul, 9 => MathFround, 10 => MathSign,
);

script_handler! {
    /// A `Call` site whose callee was the Math function of row `ROW` (`native_call_handler`): its
    /// value of Number arguments in place (`number::math_inline_call`). Any other callee or
    /// argument takes `lane_call_math`.
    fn lane_call_math_native<ROW: u8>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let native = const { math_row_native(ROW) };
        let value = if callee_is_native(vm, r.get(i.b), native) {
            // SAFETY: the call's window is registers of the running function.
            super::number::math_inline_call(native, unsafe { lane_call_window(r, i) })
        } else {
            None
        };
        let Some(value) = value else {
            sibling!(lane_call_math::<H>, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// The `GetField` of `Math.f(x)`: the field read, the argument's `Move` into the call's
    /// window and the call of Math row `ROW` (`lane_call_math_native`) as one dispatch, each
    /// register written as the three records write it (`install_math_field_call`). A read the
    /// hint misses takes the field's own refresh; another callee or argument, the `Move`
    /// record, so the call's own handler decides.
    fn lane_get_field_math_call<ROW: u8>(cx, r, ip, i, view) {
        let Some(callee) = own_bound_value::<H, false, HINTED_FORM>(cx, r, i) else {
            sibling!(lane_get_field_own_refresh::<H, false, true>, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, callee);
        let move_ip = following(ip);
        // SAFETY: the installer matched the `Move` and the `Call` that follow in this view.
        let (copy, call_ip) = (unsafe { &*move_ip }, following(move_ip));
        let call = unsafe { &*call_ip };
        let native = const { math_row_native(ROW) };
        let argument = r.get(copy.b);
        let value = if callee_is_native(unsafe { vm(cx) }, callee, native) {
            super::number::math_inline_call(native, std::slice::from_ref(&argument))
        } else {
            None
        };
        let Some(value) = value else {
            next!(cx, r, move_ip, Carried::script_in(cx, view))
        };
        r.set(copy.a, argument);
        r.set(call.a, value);
        next!(cx, r, following(call_ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `lane_get_field_math_call` of a two-argument call (`Math.min(i, 777)`): the field read,
    /// the first argument's `Move`, the second's `Move` (or `LoadConst` where `CONST_SECOND`)
    /// and the call as one dispatch, each register written as the four records write it.
    fn lane_get_field_math_call2<ROW: u8, CONST_SECOND: bool>(cx, r, ip, i, view) {
        let Some(callee) = own_bound_value::<H, false, HINTED_FORM>(cx, r, i) else {
            sibling!(lane_get_field_own_refresh::<H, false, true>, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, callee);
        let first_ip = following(ip);
        let second_ip = following(first_ip);
        let call_ip = following(second_ip);
        // SAFETY: the installer matched the copies and the `Call` that follow in this view.
        let (first, second, call) = unsafe { (&*first_ip, &*second_ip, &*call_ip) };
        let native = const { math_row_native(ROW) };
        let arguments = [r.get(first.b), if CONST_SECOND { second.constant() } else { r.get(second.b) }];
        let value = if callee_is_native(unsafe { vm(cx) }, callee, native) {
            super::number::math_inline_call(native, &arguments)
        } else {
            None
        };
        let Some(value) = value else {
            next!(cx, r, first_ip, Carried::script_in(cx, view))
        };
        r.set(first.a, arguments[0]);
        r.set(second.a, arguments[1]);
        r.set(call.a, value);
        next!(cx, r, following(call_ip), Carried::script_in(cx, view))
    }
}

/// Fuses the call `call_ip`, which `native_call_handler` just gave Math row handler `handler`,
/// with the `GetField` of its callee and the `Move` of its one argument before it
/// (`lane_get_field_math_call`): the field record a register's bound own read, writing the
/// callee register; the copy a plain `Move` into the call's one-value window.
pub(super) fn install_math_field_call<H: Host>(cx: *mut LaneContext<H>, call_ip: Ip, native: Native) {
    let Some(fused) = math_field_call_handler::<H>(native) else { return };
    // SAFETY: `call_ip` is a record of the running view, which starts at the context's base.
    let pc = unsafe { call_ip.offset_from((*cx).base) };
    if pc < 2 {
        return;
    }
    // SAFETY: as below; a two-argument call has its field record three before it.
    let call = unsafe { &*call_ip };
    if ImmediateLayout::argument_count(call.imm()) == 2 {
        if pc >= 3 {
            install_math_field_call2::<H>(call_ip, native);
        }
        return;
    }
    // SAFETY: both records precede the call in the same view.
    let (copy_ip, field_ip) = unsafe { (call_ip.sub(1), call_ip.sub(2)) };
    let (call, copy, field) = unsafe { (&*call_ip, &*copy_ip, &*field_ip) };
    let one_argument = ImmediateLayout::argument_count(call.imm()) == 1
        && ImmediateLayout::call_window_base(call.imm()) == copy.a;
    let copies = copy.handler == lane_row::<H, { opc::Move }, ACC_NONE, FLAG_UNREAD> as Handler as *const ();
    let reads_callee = field.a == call.b
        && (field.handler == lane_get_field_own::<H, false> as *const () || own_bound_handlers::<H>()[1..].iter().step_by(2).any(|&handler| handler == field.handler));
    if one_argument && copies && reads_callee && copy.a != field.a {
        requicken::<H>(field_ip, fused);
    }
}

/// `install_math_field_call` of a two-argument call: a field record, a `Move` of the first
/// argument and a `Move` or `LoadConst` of the second into the call's two-value window.
fn install_math_field_call2<H: Host>(call_ip: Ip, native: Native) {
    // SAFETY: the three records precede the call in the same view (the caller checked).
    let (second_ip, first_ip, field_ip) = unsafe { (call_ip.sub(1), call_ip.sub(2), call_ip.sub(3)) };
    let (call, second, first, field) = unsafe { (&*call_ip, &*second_ip, &*first_ip, &*field_ip) };
    let window = ImmediateLayout::call_window_base(call.imm());
    let copy = lane_row::<H, { opc::Move }, ACC_NONE, FLAG_UNREAD> as Handler as *const ();
    let constant_second = second.handler == lane_row::<H, { opc::LoadConst }, ACC_NONE, FLAG_UNREAD> as Handler as *const ();
    let shaped = first.handler == copy && (second.handler == copy || constant_second)
        && first.a == window && second.a == window + 1;
    let reads_callee = field.a == call.b && field.a != window && field.a != window + 1
        && (field.handler == lane_get_field_own::<H, false> as *const () || own_bound_handlers::<H>()[1..].iter().step_by(2).any(|&handler| handler == field.handler));
    if shaped && reads_callee && let Some(fused) = math_field_call2_handler::<H>(native, constant_second) {
        requicken::<H>(field_ip, fused);
    }
}

script_handler! {
    /// A `Call` site whose callee was the native of builtin row `ROW` (`native_call_handler`): that
    /// call in its steady case (`Vm::steady_native_value`). Any other callee or case takes
    /// `lane_native_call`.
    fn lane_call_builtin<ROW: u8>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let native = const { row_native(ROW) };
        let value = if !callee_is_native(vm, r.get(i.b), native) {
            None
        } else if const { matches!(row_native(ROW), Native::RegExpTest | Native::RegExpExec) } {
            // A RegExp method's steady call (`Vm::steady_regexp_call`), entered from the site
            // without the general native protocol; an error it raises completes as
            // `lane_native_call`'s does.
            // SAFETY: the call's window is registers of the running function; the context's
            // program outlives the lane run.
            let (args, code) = unsafe { (lane_call_window(r, i), &*(*cx).code) };
            let result_used = ImmediateLayout::lane_call_result_used(i.imm());
            match args.first().and_then(|&subject| vm.steady_regexp_call(code, native, r.get(i.c), subject, result_used)) {
                Some(Ok(value)) => Some(value),
                Some(Err(error)) => {
                    note_allocation(vm);
                    vm.lane_raised = Some(error);
                    resume!(cx, lane_native_complete(cx, ip, i, None))
                }
                None => None,
            }
        } else {
            // SAFETY: the call's window is registers of the running function.
            vm.steady_native_value(native, r.get(i.c), unsafe { lane_call_window(r, i) })
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
        native if let Some(handler) = math_handler::<H>(native) => handler,
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
pub(super) fn lane_body_native_call<H: Host>(cx: *mut LaneContext<H>, call: &super::body_forms::BodyCall, callee: Value, receiver: Value) -> u64 {
    use super::super::compiled::LANE_CALL_COMPLETED;
    let vm = unsafe { vm(cx) };
    // A returning call's completion is its activation's, which the lane makes.
    if call.site.returns { return super::lane_body_stats::miss(vm, super::lane_body_stats::Miss::Returning); }
    let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = vm.heap.function(callee) else { return super::lane_body_stats::miss(vm, super::lane_body_stats::Miss::NativeNotNative) };
    let native = *native;
    // SAFETY: the context's program outlives the lane run; the values are registers of the
    // running function, which the native reads before anything can move them.
    let (code, args) = unsafe { (&*(*cx).code, call.values()) };
    let Some(value) = vm.in_place_native_value(code, native, callee, receiver, &args) else {
        return lane_body_native_out(cx, call, native, (callee, receiver), &args);
    };
    note_allocation(vm);
    call.registers.set(call.site.result, value);
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
                match lane_activate_getter(cx, r, ip, getter, receiver, None) {
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
fn lane_body_native_out<H: Host>(cx: *mut LaneContext<H>, call: &super::body_forms::BodyCall, native: Native,
    (callee, receiver): (Value, Value), args: &[Value]) -> u64 {
    use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_LEFT};
    let ip = call.ip;
    // An array callback method's callbacks are activations of the lane (`lane_begin_array_iteration`).
    if native.is_array_callback() && let Some(status) = lane_body_array_iteration(cx, ip, native, receiver, args) {
        return status;
    }
    let Some(value) = lane_call_native(cx, ip, native, callee, receiver, args) else {
        return super::lane_body_stats::miss(unsafe { vm(cx) }, super::lane_body_stats::Miss::NativeNotCompleted);
    };
    // Guest code the native ran may have made the activations frames, whose registers these are.
    let after = unsafe { (*cx).registers };
    after.set(call.site.result, value);
    if after.0 == call.registers.0 { return LANE_CALL_COMPLETED; }
    unsafe { (*cx).callee_resume = following(ip); (*cx).callee_acc = Value::UNDEFINED; }
    LANE_CALL_LEFT
}
