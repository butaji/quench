//! Calls whose arguments come from a list: applied calls and spread calls.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// `f.apply(thisArg, list)` (`native` `FunctionApply`, `f` the receiver) or
/// `Reflect.apply(f, thisArg, list)` (`ReflectApply`) of a frameless `f` and a list that is a
/// dense array without holes, or absent for `apply`: `f`'s activation with the list's
/// elements, which CreateListFromArrayLike reads as its own data elements and the activation
/// copies before anything runs, as `function_call_activation_in_lane` activates a call's. `None`
/// for any other call, which the native makes.
#[inline(never)]
pub(crate) fn apply_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
) -> Option<Result<(Registers, Ip), Ip>> {
    // SAFETY: the call's window is registers of the running function.
    let args = unsafe { call_window_in_lane(r, i) };
    let argument = |index: usize| args.get(index).copied().unwrap_or(Value::UNDEFINED);
    let (callee, receiver, list) = match native {
        Native::FunctionApply => (r.get(i.c), argument(0), argument(1)),
        _ => (argument(0), argument(1), *args.get(2)?),
    };
    #[cfg(feature = "profile-lane-exits")]
    {
        let vm = unsafe { vm(cx) };
        super::lane_exits::record_detail(format!(
            "apply callee={:?} array={} arguments_object={} sparse={} holes={} activations={}",
            vm.function_record(callee).map(|record| match record.kind {
                FunctionKind::User(..) => "user",
                FunctionKind::Native(_) => "native",
            }),
            vm.heap_reads().array(list).is_some(),
            vm.heap_reads().array(list).is_some_and(|array| array.is_arguments_object()),
            vm.heap.sparse_length(list).is_some(),
            vm.heap_reads().array(list).is_some_and(|array| array.elements().as_slice().iter().any(|value| value.is_deleted())),
            vm.lane_stack.activation_count(),
        ));
    }
    let absent = native == Native::FunctionApply && (list.is_undefined() || list.is_null());
    list_activation_in_lane(cx, r, ip, i, callee, receiver, list, if absent { None } else { Some(unsafe { vm(cx) }.dense_list_length(list)?) })
}

/// `callee`'s activation with `receiver` and the elements of `list`, a dense array without
/// holes whose first `listed` stored elements are the arguments (`Vm::dense_list_length`;
/// none when `None`), for an applied call or a spread call (`CallArray`), whose
/// argument array the call's spread evaluation built; `None` for any other list.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
pub(crate) fn list_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    list: Value,
    listed: Option<usize>,
) -> Option<Result<(Registers, Ip), Ip>> {
    let vm = unsafe { vm(cx) };
    let length = listed.unwrap_or(0);
    // A bound function or `Function.prototype.call` forwards the list to the function it
    // names (`Vm::forwarded_call`), which a forwarded call activates; the elements are copied
    // out first, as a frame call's are.
    if matches!(
        vm.function_record(callee),
        Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionBoundCall | Native::FunctionCall), .. })
    ) {
        let elements: smallvec::SmallVec<[Value; FRAME_CALL_INLINE_ARGUMENTS]> = vm.heap_reads().array_elements(list).map_or_else(smallvec::SmallVec::new, |elements| elements.range(0..length).collect());
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
        let Some(activated) = passed_target_in_lane(cx, target, args.len()) else {
            return Some(Err(passed_frame_call_in_lane(cx, r, ip, i, target, receiver, FrameCallArguments::List(args)).into_loop()));
        };
        let Some(this) = call_this_in_lane(vm, &activated, receiver) else {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::record_detail("applied forwarding this declined".into());
            return Some(Err(exit(ip)));
        };
        return Some(activate_passed_in_lane(cx, r, ip, &activated, args, this));
    }
    // The site's `CallOperand::memo` slot links the callee's target (`forwarded_target_in_lane`).
    let mut resolved = None;
    let Some(target) = forwarded_target_in_lane(cx, ip, callee, length, &mut resolved).copied() else {
        // A user function that needs a frame takes it in place (`passed_frame_call_in_lane`), with
        // the elements copied out first: making the caller's
        // activations frames moves no heap array, but the callee may change the list.
        let elements: smallvec::SmallVec<[Value; FRAME_CALL_INLINE_ARGUMENTS]> =
            vm.heap_reads().array_elements(list).map_or_else(smallvec::SmallVec::new, |elements| elements.range(0..length).collect());
        return Some(Err(passed_frame_call_in_lane(cx, r, ip, i, callee, receiver, FrameCallArguments::List(&elements)).into_loop()));
    };
    let Some(this) = call_this_in_lane(vm, &target, receiver) else {
        return Some(Err(exit(ip)));
    };
    // Read after the resolution above, which runs no guest code but may allocate.
    let held = if listed.is_some() { vm.heap_reads().array_elements(list)?.values_cow() } else { std::borrow::Cow::Borrowed(&[][..]) };
    let elements: &[Value] = if listed.is_some() { &held[..length] } else { &[] };
    Some(activate_passed_in_lane(cx, r, ip, &target, elements, this))
}

/// `call_spread_miss` of a native other than a forwarding one or `eval`: the native's call
/// in place with the array's elements, copied out first, as a forwarded call passes its
/// values (`native_value_in_lane`). A lane call record carries no eval flag (`call_fits_in_lane`),
/// so one whose result is unused (`ImmediateLayout::call_result_used_in_lane`) is made too: the
/// value goes to its dead register. `None` for any other callee.
#[inline(never)]
pub(crate) fn spread_native_call_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    list: Value,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let native = spread_native_in_lane(vm, callee)?;
    if i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 && !returning_call_admitted(cx) {
        return Some(Resumed::Exit(exit(ip)));
    }
    let args = vm.heap_reads().array_elements(list)?.iter().collect::<smallvec::SmallVec<[Value; FRAME_CALL_INLINE_ARGUMENTS]>>();
    let value = native_value_in_lane(cx, ip, i, native, callee, receiver, &args);
    Some(native_complete_in_lane(cx, ip, i, value))
}

/// The native `spread_native_call_in_lane` calls in place: any but a forwarding one or `eval`.
#[inline(always)]
fn spread_native_in_lane(vm: &Vm, callee: Value) -> Option<Native> {
    let FunctionKind::Native(native) = vm.function_record(callee)?.kind else {
        return None;
    };
    (!matches!(native, Native::FunctionCall | Native::FunctionBoundCall | Native::FunctionApply | Native::ReflectApply | Native::Eval)).then_some(native)
}

/// `spread_native_call_in_lane` of a spread of a Set that iterates intact
/// (`Vm::intact_iteration_kind`), as Vue's effect trigger spreads a dependency's Set into
/// `push`: its live values, which iterating it yields and no step's code can change, copied
/// out first. `None` for any other spread or callee, which the general path makes.
#[inline(never)]
pub(crate) fn spread_set_native_call_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    list: Value,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    if !vm.heap_reads().is_set(list) {
        return None;
    }
    let native = spread_native_in_lane(vm, callee)?;
    if vm.intact_iteration_kind(list) != Some(quench_runtime::vm::iterators::IterationSource::Set) {
        return None;
    }
    if i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 && !returning_call_admitted(cx) {
        return Some(Resumed::Exit(exit(ip)));
    }
    let args: smallvec::SmallVec<[Value; FRAME_CALL_INLINE_ARGUMENTS]> = vm.heap_reads().set_entries(list)?.iter().collect();
    let value = native_value_in_lane(cx, ip, i, native, callee, receiver, &args);
    Some(native_complete_in_lane(cx, ip, i, value))
}
