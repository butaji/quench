//! Arguments of a lane activation: the kept and unmaterialized argument views a body reads, and the constructing check of a call.
use super::*;

/// The values the running function kept for its forwarded `arguments`: a forwarding
/// activation's, after its registers, or a frame's `original_arguments`.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(in super::super) unsafe fn kept_arguments_in_lane(cx: *mut LaneContext) -> Option<ActivationArguments> {
    let vm = unsafe { vm(cx) };
    if unsafe { (*cx).registers }.env().is_deleted() {
        let frame = &vm.frames[unsafe { (*cx).frame() }];
        return Some(ActivationArguments::Kept(
            frame.original_arguments.as_ptr(),
            frame.original_arguments.len(),
        ));
    }
    let activation = vm.lane_stack.top_frame()?;
    let script = activation.script().filter(|script| script.forwards)?;
    let registers = usize::from(unsafe { &*(*cx).code }.function(activation.function() as usize).registers);
    // SAFETY: the activation's window holds its registers, then its kept values.
    let values = unsafe { activation.window_ptr().add(registers) };
    Some(ActivationArguments::Kept(values, usize::from(script.argc)))
}

/// The values the running function kept for its unmaterialized `arguments` binding, in the
/// local slot `slot` (`kept_arguments_in_lane`): `None` where a frame's binding was
/// materialized, which the general path then reads.
///
/// # Safety
/// `cx` is the running lane's context, and `slot` its function's validated local slot.
#[inline(always)]
pub(in super::super) unsafe fn unmaterialized_arguments_in_lane(
    cx: *mut LaneContext,
    slot: usize,
) -> Option<(*const Value, usize)> {
    if unsafe { (*cx).registers }.env().is_deleted()
        && unsafe { vm(cx).read_validated_local((*cx).frame(), slot) }
            != Value::UNMATERIALIZED_ARGUMENTS
    {
        return None;
    }
    match unsafe { kept_arguments_in_lane(cx) }? {
        ActivationArguments::Kept(values, count) => Some((values, count)),
        ActivationArguments::Window(_) => None,
    }
}

/// The running keeping activation's kept-value count and its function, when the lane runs
/// one (`ScriptActivation::forwards`): its values follow its registers in its window, which
/// `r` names. `None` for a frame or any other activation.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(in super::super) unsafe fn keeping_activation_in_lane(cx: *mut LaneContext) -> Option<(u16, u32)> {
    if unsafe { (*cx).registers }.env().is_deleted() {
        return None;
    }
    let activation = unsafe { vm(cx) }.lane_stack.top_frame()?;
    let script = activation.script().filter(|script| script.forwards)?;
    Some((script.argc, activation.function()))
}

/// The count of the running function's `arguments` binding in local `slot`, for the
/// `ArgumentsLength` row (`Frame::arguments_length`): a keeping activation's kept-value count.
/// `KEPT`: the view proved `slot` its function's kept arguments (`Function::kept_arguments`).
/// `None` leaves the count to the row's general step, which reads the frame's or window's binding.
///
/// # Safety
/// `cx` is the running lane's context, and `slot` its function's validated local slot.
#[inline(always)]
pub(in super::super) unsafe fn arguments_count<const KEPT: bool>(cx: *mut LaneContext, slot: u16) -> Option<Value> {
    if let Some((argc, function)) = unsafe { keeping_activation_in_lane(cx) }
        // SAFETY: the context's program outlives the lane run.
        && (KEPT || unsafe { &*(*cx).code }.function(function as usize).arguments_slot == Some(slot))
    {
        return Some(Value::integer(i32::from(argc)));
    }
    None
}

/// The keeping activation's `arguments` binding a local read of its slot takes (`Frame::arguments_binding`): a
/// frame run reads its own binding as the general path does (`Vm::read_validated_local`, which materializes it);
/// a frameless activation's is the unmaterialized marker a forwarding read passes on (`OBJECT` false), or the
/// object made from the values it kept (`OBJECT` true, `Vm::activation_arguments_object`), which the keeping
/// activation's last read makes. `None` where the object cannot be made: the row leaves to its miss.
/// Subsumes the hand loads `lane_load_forwarded_arguments` and `lane_load_kept_arguments_object` of the lane.
///
/// # Safety
/// `cx` is the running lane's context, and `r` its register file.
pub(in super::super) unsafe fn kept_arguments_binding<const OBJECT: bool>(cx: *mut (), r: Registers, slot: u16) -> Option<Value> {
    let cx = cx.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    // The forwarding check: a frame run's registers hold the deleted environment marker.
    if unsafe { (*cx).registers }.env().is_deleted() {
        // SAFETY: validated bytecode bounds the slot by Function.locals, and the frame is the running one.
        return Some(unsafe { vm.read_validated_local((*cx).frame(), usize::from(slot)) });
    }
    if !OBJECT {
        return Some(Value::UNMATERIALIZED_ARGUMENTS);
    }
    let Some((function, Some(script))) =
        vm.lane_stack.top_frame().map(|activation| (activation.function(), activation.script()))
    else {
        return None;
    };
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function(function as usize);
    // A keeping activation's values follow its registers (`activate_keeping_in_lane`).
    let kept = (0..script.argc)
        .map(|index| r.get(body.registers + index))
        .collect::<SmallVec<[Value; GATHERED_INLINE_VALUES]>>();
    let context = CallContext::user_function(function, script.callee);
    let arguments = vm.activation_arguments_object(body, context, &kept).ok()?;
    note_allocation(vm);
    Some(arguments)
}

/// The frameless target of a lane call to `callee`, resolved on first use.
/// Whether `callee` is the target of a `new` whose `new.target` the general path still
/// holds (`Vm::construct_target`): a lane activation would not bind it.
#[inline(always)]
pub(in super::super) fn constructing_in_lane(vm: &Vm, callee: Value) -> bool {
    vm.construct_target
        .is_some_and(|target| target.callee == callee)
}
