//! Calls whose arguments come from a list: applied calls and spread calls.
use super::*;

/// `f.apply(thisArg, list)` (`native` `FunctionApply`, `f` the receiver) or
/// `Reflect.apply(f, thisArg, list)` (`ReflectApply`) of a frameless `f` and a list that is a
/// dense array without holes, or absent for `apply`: `f`'s activation with the list's
/// elements, which CreateListFromArrayLike reads as its own data elements and the activation
/// copies before anything runs, as `lane_function_call_activation` activates a call's. `None`
/// for any other call, which the native makes.
#[inline(never)]
pub(super) fn lane_apply_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
) -> Option<Result<(Registers, Ip), Ip>> {
    // SAFETY: the call's window is registers of the running function.
    let args = unsafe { lane_call_window(r, i) };
    let argument = |index: usize| args.get(index).copied().unwrap_or(Value::UNDEFINED);
    let (callee, receiver, list) = match native {
        Native::FunctionApply => (r.get(i.c), argument(0), argument(1)),
        _ => (argument(0), argument(1), *args.get(2)?),
    };
    let listed = !(native == Native::FunctionApply && (list.is_undefined() || list.is_null()));
    #[cfg(feature = "profile-lane-exits")]
    {
        let vm = unsafe { vm(cx) };
        super::lane_exits::record_detail(format!(
            "apply callee={:?} array={} arguments_object={} sparse={} holes={} activations={}",
            vm.heap.function(callee).map(|record| match record.kind {
                FunctionKind::User(..) => "user",
                FunctionKind::Native(_) => "native",
            }),
            vm.heap.array(list).is_some(),
            vm.heap.array(list).is_some_and(|array| array.is_arguments_object()),
            vm.heap.sparse_length(list).is_some(),
            vm.heap.array(list).is_some_and(|array| array.elements().as_slice().iter().any(|value| value.is_deleted())),
            vm.lane_stack.activations.len(),
        ));
    }
    lane_list_activation(cx, r, ip, i, callee, receiver, list, listed)
}

/// `callee`'s activation with `receiver` and the elements of `list`, a dense array without
/// holes (none when not `listed`), for an applied call or a spread call (`CallArray`), whose
/// argument array the call's spread evaluation built; `None` for any other list.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
pub(super) fn lane_list_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    list: Value,
    listed: bool,
) -> Option<Result<(Registers, Ip), Ip>> {
    let vm = unsafe { vm(cx) };
    let dense = |vm: &Vm<H>| {
        vm.heap.array(list).is_some_and(|array| {
            !array.is_arguments_object()
                && vm.heap.sparse_length(list).is_none()
                && !array.elements().as_slice().iter().any(|value| value.is_deleted())
        })
    };
    if listed && !dense(vm) {
        return None;
    }
    // A bound function or `Function.prototype.call` forwards the list to the function it
    // names (`Vm::forwarded_call`), which a forwarded call activates; the elements are copied
    // out first, as a frame call's are.
    if matches!(
        vm.heap.function(callee),
        Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionBoundCall | Native::FunctionCall), .. })
    ) {
        let elements = if listed {
            vm.heap.array_elements(list)?.as_slice().iter().copied().collect()
        } else {
            smallvec::SmallVec::<[Value; FRAME_CALL_INLINE_ARGUMENTS]>::new()
        };
        let Some(super::function::Forwarded { callee: target, receiver, args, kind, .. }) = vm.forwarded_call(callee, receiver, elements) else {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::record_detail("applied forwarding declined".into());
            return None;
        };
        let args = args.as_slice();
        if !matches!(kind, Some(FunctionKind::User(..))) {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::record_detail("applied forwarding to a native".into());
            return None;
        }
        let Some(activated) = lane_passed_target(cx, target, args.len()) else {
            return Some(Err(lane_passed_frame_call(cx, r, ip, i, target, receiver, FrameCallArguments::List(args)).into_loop()));
        };
        let Some(this) = lane_call_this(vm, &activated, receiver) else {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::record_detail("applied forwarding this declined".into());
            return Some(Err(exit(ip)));
        };
        return Some(lane_activate_passed(cx, r, ip, &activated, args, this));
    }
    let length = if listed { vm.heap.array(list).map_or(0, |array| array.elements().len()) } else { 0 };
    let Some(target) = lane_passed_target(cx, callee, length) else {
        // A user function that needs a frame takes it in place (`lane_passed_frame_call`), with
        // the elements copied out first: making the caller's
        // activations frames moves no heap array, but the callee may change the list.
        let elements = if listed {
            vm.heap.array_elements(list)?.as_slice().iter().copied().collect()
        } else {
            smallvec::SmallVec::<[Value; FRAME_CALL_INLINE_ARGUMENTS]>::new()
        };
        return Some(Err(lane_passed_frame_call(cx, r, ip, i, callee, receiver, FrameCallArguments::List(&elements)).into_loop()));
    };
    let Some(this) = lane_call_this(vm, &target, receiver) else {
        return Some(Err(exit(ip)));
    };
    // Read after the resolution above, which runs no guest code but may allocate.
    let elements: &[Value] = if listed { vm.heap.array_elements(list)?.as_slice() } else { &[] };
    Some(lane_activate_passed(cx, r, ip, &target, elements, this))
}

/// `lane_call_spread` of a native other than a forwarding one or `eval`, whose record's
/// eval flag would read as an unused result: the native's call in place with the array's
/// elements, copied out first, as a forwarded call passes its values (`lane_native_value`).
/// `None` for any other callee.
#[inline(never)]
pub(super) fn lane_spread_native_call<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    list: Value,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let FunctionKind::Native(native) = vm.heap.function(callee)?.kind else {
        return None;
    };
    if matches!(native, Native::FunctionCall | Native::FunctionBoundCall | Native::FunctionApply | Native::ReflectApply | Native::Eval)
        || !ImmediateLayout::lane_call_result_used(i.imm())
    {
        return None;
    }
    if i.a & crate::bytecode::RETURN_REGISTER != 0 && !lane_returning_call_admitted(cx) {
        return Some(Resumed::Exit(exit(ip)));
    }
    let args = smallvec::SmallVec::<[Value; FRAME_CALL_INLINE_ARGUMENTS]>::from_slice(vm.heap.array_elements(list)?.as_slice());
    let value = lane_native_value(cx, ip, i, native, callee, receiver, &args);
    Some(lane_native_complete(cx, ip, i, value))
}
