//! Array callback methods (`forEach`, `map`, the finds, the reducers) run as their
//! callbacks' activations in the calling lane, each return stepping the loop
//! (`lane_begin_array_iteration`).
use super::*;
#[cfg(test)]
mod tests;

/// The values an array callback method passes its callback: the element, its index and the
/// array, after a reducer's accumulator (`Native::is_array_reducer`).
fn array_callback_argument_count(native: Native) -> usize {
    const ELEMENT_INDEX_ARRAY: usize = 3;
    ELEMENT_INDEX_ARRAY + usize::from(native.is_array_reducer())
}

/// An array callback method's iteration a lane runs (`lane_begin_array_iteration`), one
/// entry per `Completion::ArrayStep` activation, innermost last: the loop's state and what
/// entering its callback's activation again for the next element reads.
#[derive(Clone, Copy)]
pub(super) struct LaneArrayStep {
    pub(super) iteration: super::super::array_indexed::ArrayIteration,
    /// The element the running callback was passed, at `index`: a filter keeps it, a find
    /// answers it.
    pub(super) element: Value,
    pub(super) index: usize,
    pub(super) entry: PreparedEntry,
}

/// An array callback method's callback and `thisArg` among the values it was passed; a
/// reducer's second value is its initial accumulator, and its callback's `thisArg` is
/// `undefined`.
#[inline(always)]
fn array_callback_arguments(native: Native, args: &[Value]) -> Option<(Value, Value)> {
    let this_arg = if native.is_array_reducer() { None } else { args.get(1).copied() };
    Some((*args.first()?, this_arg.unwrap_or(Value::UNDEFINED)))
}

/// The activation of an array callback method `native`'s `callback` called with `this_arg`
/// from the call record `ip`, passed its values (`array_callback_argument_count`),
/// completing as `completion` says: its shape and entry (`PreparedShape`). `None` for a
/// callback no lane call activates.
#[inline(always)]
fn lane_array_callback_activation<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    native: Native,
    callback: Value,
    this_arg: Value,
    completion: Completion,
) -> Option<(PreparedShape, PreparedEntry)> {
    let mut resolved = None;
    let target = lane_callback_target(cx, ip, callback, &mut resolved)?;
    let this = lane_call_this(unsafe { vm(cx) }, target, this_arg)?;
    // SAFETY: the running lane's context, which nothing else accesses.
    let mut shape = PreparedShape::of(unsafe { &*cx }, ip, target, array_callback_argument_count(native))?;
    if let ActivationKind::Script(script) = &mut shape.activation.kind {
        script.completion = completion;
    }
    let entry = shape.entry(target, this);
    Some((shape, entry))
}

/// `lane_script_target_in` of an array callback method's `callback` at its call record `ip`,
/// whose read memo slot (`CallOperand::memo`), which no native's call reads, links the
/// callback's cache entry as a script call's `call_link` links its callee's: the link, tested
/// first, skips the set's hash. The record's `call_link` stays its own callee's, which a lane
/// body reads (`lane_site_targets`); a read memo the slot named once is another site's or
/// this one's, as `site_read_memo` tells.
#[inline(always)]
fn lane_callback_target<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    callback: Value,
    resolved: &mut Option<ScriptCallTarget>,
) -> Option<&ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    // SAFETY: a call record's operand is `call`.
    let link = unsafe { (*ip).operand.call.memo } as usize;
    if vm.script_call_holds(link, callback) && !lane_constructing(vm, callback) {
        // SAFETY: `script_call_holds` found the entry within the cache, which nothing
        // resolves into before the caller reads it.
        let target = unsafe { &*std::ptr::from_ref(vm.lane_stack.script_targets.entries.get_unchecked(link)) };
        return lane_admits_activation(target).then_some(target);
    }
    let target = lane_script_target_in(cx, callback, resolved)?;
    if let Some(index) = vm.script_call_index(callback) {
        // SAFETY: as `requicken`: the record's operand is the call's, written in place.
        unsafe { (*ip.cast_mut()).operand.call.memo = index as u32 };
    }
    Some(target)
}

/// Whether a target of the running program activates in the lane from a path besides a
/// plain call (`lane_script_target_in`): not a frame, an environment or a settling one.
#[inline(always)]
pub(super) fn lane_admits_activation(target: &ScriptCallTarget) -> bool {
    !matches!(target.entry, CallEntry::Frame | CallEntry::Environment | CallEntry::Settles)
}

/// `forEach` at the lane `Call` record `ip`, whose result nothing reads
/// (`lane_call_result_dead`), over a dense array `receiver` whose storage holds one element (the
/// caller tested it), with `args`: where the element is present, the callback's one call with
/// it, its index and the array, a plain activation (`Completion::Plain`) whose value the dead
/// register takes. The method's own value, `undefined`, is never read, and the length it read
/// bounds its loop to that call. `None` leaves the call to `lane_begin_array_iteration`.
#[inline(never)]
pub(super) fn lane_call_for_each_once<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    receiver: Value,
    args: &[Value],
) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } || vm.with_scope_entered || vm.own_array_length(receiver) != Some(1) {
        return None;
    }
    let element = lane_dense_element(vm, receiver, 0)?;
    let (callback, this_arg) = array_callback_arguments(Native::ArrayForEach, args)?;
    let (shape, entry) = lane_array_callback_activation(cx, ip, Native::ArrayForEach, callback, this_arg, Completion::Plain)?;
    let window = lane_admit_prepared(cx, &shape.activation, shape.room)?;
    // SAFETY: the admission pushed the activation over the window.
    let start = unsafe { lane_enter_prepared_values(cx, &entry, [element, Value::length(0), receiver], window) };
    // SAFETY: the entry made the callback's registers the context's.
    Some((unsafe { (*cx).registers }, start))
}

/// The call of an array callback method `native` at the lane `Call` record `ip` over a dense
/// array `receiver` with `args`, run in the lane as the bytecode JSC's builtin runs: each
/// element's callback is an activation of the caller's lane (`Completion::ArrayStep`) whose
/// return steps the loop and enters the activation again for the next element
/// (`lane_step_array_iteration_in_place`), with no native frame and no nested run. `None`
/// leaves the call to the native: another receiver, a callback no lane call activates, or a
/// result the species makes.
#[inline(never)]
pub(super) fn lane_begin_array_iteration<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    native: Native,
    receiver: Value,
    args: &[Value],
) -> Option<Resumed> {
    let (mut iteration, shape, entry) = lane_array_iteration_setup(cx, ip, native, receiver, args)?;
    let vm = unsafe { vm(cx) };
    let Some(index) = iteration.advance() else {
        // SAFETY: `ip` is the call record of the running function's view.
        return Some(lane_native_complete(cx, ip, unsafe { &*ip }, Some(iteration.completion())));
    };
    if let Some(element) = lane_dense_element(vm, receiver, index)
        && let Some(window) = lane_admit_prepared(cx, &shape.activation, shape.room)
    {
        // SAFETY: the admission pushed the activation over the window.
        let start = unsafe { lane_enter_array_step::<H, false>(cx, &entry, &iteration, element, index, window) };
        vm.lane_stack.steps.push(LaneArrayStep { iteration, element, index, entry });
        // SAFETY: the entry made the callback's registers the context's.
        return Some(Resumed::At(unsafe { (*cx).registers }, start, Carried::script(cx)));
    }
    iteration.visited -= 1;
    Some(lane_resume_array_iteration(cx, ip, iteration))
}

/// `lane_begin_array_iteration`'s setup: the iteration, with a reducer's accumulator, and
/// its callback's activation. `None` where the lane leaves the call to the native.
#[inline(always)]
fn lane_array_iteration_setup<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    native: Native,
    receiver: Value,
    args: &[Value],
) -> Option<(super::super::array_indexed::ArrayIteration, PreparedShape, PreparedEntry)> {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } || vm.with_scope_entered {
        return None;
    }
    let length = lane_array_object(vm, receiver)?.elements().len();
    if vm.own_array_length(receiver) != Some(length) {
        return None;
    }
    let (callback, this_arg) = array_callback_arguments(native, args)?;
    let (shape, entry) = lane_array_callback_activation(cx, ip, native, callback, this_arg, Completion::ArrayStep)?;
    let mut iteration = vm.fresh_array_iteration(native, receiver, callback, this_arg, length)?;
    note_allocation(vm);
    // A reducer without an initial value starts from its first element, which a dense
    // array holds present; anything else leaves the call to the native.
    if native.is_array_reducer() {
        iteration.result = match args.get(1) {
            Some(initial) => *initial,
            None => lane_dense_element(vm, receiver, iteration.advance()?)?,
        };
    }
    Some((iteration, shape, entry))
}

/// The array `receiver` names: a direct array's record, found inline as
/// `Vm::own_array_length` finds it, or a slot's.
#[inline(always)]
pub(super) fn lane_array_object<H: Host>(vm: &Vm<H>, receiver: Value) -> Option<&crate::heap::ArrayObject> {
    match vm.heap.record_array(receiver) {
        Some(object) => Some(object),
        None => vm.heap.array(receiver),
    }
}

/// The present element at `index` of `receiver`'s dense storage: an own data property,
/// which HasProperty finds and Get reads observing nothing.
#[inline(always)]
fn lane_dense_element<H: Host>(vm: &Vm<H>, receiver: Value, index: usize) -> Option<Value> {
    let element = *lane_array_object(vm, receiver)?.elements().get(index)?;
    (!element.is_deleted()).then_some(element)
}

/// The iteration `iteration` of the call at `ip`, whose caller runs, past its last element:
/// the call completes with the method's value; else the rest of its loop on the native's
/// path from the caller's frame (`Vm::array_iteration_run`), which reads holes and anything
/// else generically.
#[inline(always)]
fn lane_resume_array_iteration<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    iteration: super::super::array_indexed::ArrayIteration,
) -> Resumed {
    if iteration.visited == iteration.length {
        // SAFETY: `ip` is the call record of the running function's view.
        return lane_native_complete(cx, ip, unsafe { &*ip }, Some(iteration.completion()));
    }
    lane_run_array_iteration_from_frame(cx, ip, iteration)
}

/// `lane_resume_array_iteration` of an iteration with elements left: the native's loop.
#[inline(never)]
fn lane_run_array_iteration_from_frame<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    mut iteration: super::super::array_indexed::ArrayIteration,
) -> Resumed {
    let value = lane_call_from_frame(cx, ip, |vm, code| vm.array_iteration_run(code, &mut iteration));
    // SAFETY: `ip` is the call record of the running function's view.
    lane_native_complete(cx, ip, unsafe { &*ip }, value)
}

/// The return of an array callback method's callback `activation` with `mapped`, its
/// caller restored (`lane_restore_caller`): the step of the innermost lane iteration, then
/// the rest of its loop (`lane_finish_array_step`).
pub(super) fn lane_step_array_iteration<H: Host>(cx: *mut LaneContext<H>, activation: &Activation, mapped: Value) -> Resumed {
    let applied = lane_apply_array_step(cx, mapped);
    lane_finish_array_step(cx, activation, applied)
}

/// The step of the innermost lane iteration whose callback returned `mapped`
/// (`Vm::fresh_array_step`): the method's value where the step ends it.
#[inline(always)]
fn lane_apply_array_step<H: Host>(cx: *mut LaneContext<H>, mapped: Value) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let step: *mut LaneArrayStep = vm.lane_stack.steps.last_mut().expect("each array step activation has its entry");
    // SAFETY: the entry stays in place while the step, which reads no lane stack, runs.
    let step = unsafe { &mut *step };
    let applied = vm.fresh_array_step(&mut step.iteration, step.index, step.element, mapped);
    note_allocation(vm);
    applied
}

/// `lane_step_array_iteration` of the innermost activation, a callback that returned
/// `mapped`: where the step goes on to a present dense element and no collection is due,
/// the activation is entered again for it in place, as the lane's own loop of calls would
/// make its next one (`lane_enter_prepared`); otherwise the activation retires to its
/// caller first.
///
/// The step runs inside the returning handler: a `forEach` applies nothing, and the next
/// element is read from the receiver's dense storage by its index alone.
#[inline(always)]
pub(super) fn lane_step_array_iteration_in_place<H: Host>(cx: *mut LaneContext<H>, mapped: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let step: *mut LaneArrayStep = vm.lane_stack.steps.last_mut().expect("each array step activation has its entry");
    // SAFETY: the entry stays in place while the step, which reads no lane stack, runs and
    // the callback's window is filled again.
    let step = unsafe { &mut *step };
    let applied = if step.iteration.native == Native::ArrayForEach {
        None
    } else {
        lane_apply_array_step(cx, mapped)
    };
    let depth = vm.lane_stack.activations.len() - 1;
    if applied.is_none()
        && depth < vm.lane_stack.activation_limit.get()
        && let Some(index) = step.iteration.advance()
    {
        let receiver = step.iteration.receiver;
        if let Some(element) = lane_dense_element(vm, receiver, index) {
            step.element = element;
            step.index = index;
            // The callback's completed loops close nothing more.
            vm.lane_stack.drop_iterators_from(depth);
            // SAFETY: an activation runs, the innermost, at `depth`.
            let window = unsafe { vm.lane_stack.activations.get_unchecked(depth) }.window;
            // SAFETY: the window the activation held, which its return left in place.
            let start = unsafe { lane_enter_array_step::<H, true>(cx, &step.entry, &step.iteration, element, index, window) };
            return Some(Resumed::At(unsafe { (*cx).registers }, start, Carried::script(cx)));
        }
        step.iteration.visited -= 1;
    }
    Some(lane_retire_array_step(cx, applied))
}

/// `lane_step_array_iteration_in_place` past the in-place step: the callback's activation
/// retires to its caller, then the iteration finishes (`lane_finish_array_step`).
#[inline(never)]
fn lane_retire_array_step<H: Host>(cx: *mut LaneContext<H>, applied: Option<Value>) -> Resumed {
    let vm = unsafe { vm(cx) };
    let activation = vm.lane_stack.pop_activation().expect("an array step's activation runs");
    lane_restore_caller(cx, &activation);
    lane_finish_array_step(cx, &activation, applied)
}

/// The iteration of the call `activation` made, its caller restored, after a step that
/// `applied`: the iteration retires, and the call completes with the method's value where
/// the step ended it, else goes on as `lane_resume_array_iteration` does.
#[inline(always)]
fn lane_finish_array_step<H: Host>(cx: *mut LaneContext<H>, activation: &Activation, applied: Option<Value>) -> Resumed {
    let vm = unsafe { vm(cx) };
    let depth = vm.lane_stack.steps.len() - 1;
    let iteration = vm.lane_stack.steps[depth].iteration;
    vm.lane_stack.steps.truncate(depth);
    // SAFETY: the call record lies in the restored caller's view.
    let i = unsafe { &*activation.call };
    match applied {
        None => lane_resume_array_iteration(cx, activation.call, iteration),
        value => lane_native_complete(cx, activation.call, i, value),
    }
}

/// The callback activation of `iteration`'s step passed `element` at `index`: entered
/// (`lane_enter_prepared_values`), or, `AGAIN`, entered again in the window its return left
/// (`lane_reenter_prepared_values`). The callback is passed the element, its index and the
/// array, after a reducer's accumulator.
///
/// # Safety
/// As the entry's.
#[inline(always)]
unsafe fn lane_enter_array_step<H: Host, const AGAIN: bool>(
    cx: *mut LaneContext<H>,
    entry: &PreparedEntry,
    iteration: &super::super::array_indexed::ArrayIteration,
    element: Value,
    index: usize,
    window: usize,
) -> Ip {
    let (index, receiver) = (Value::length(index), iteration.receiver);
    unsafe {
        if iteration.native.is_array_reducer() {
            let values = [iteration.result, element, index, receiver];
            if AGAIN { lane_reenter_prepared_values(cx, entry, values, window) } else { lane_enter_prepared_values(cx, entry, values, window) }
        } else {
            let values = [element, index, receiver];
            if AGAIN { lane_reenter_prepared_values(cx, entry, values, window) } else { lane_enter_prepared_values(cx, entry, values, window) }
        }
    }
}
