//! Array callback methods (`forEach`, `map`, the finds, the reducers) run as their
//! callbacks' activations in the calling lane, each return stepping the loop
//! (`begin_array_iteration_in_lane`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::array_iteration::{self as step_rules, Kind};
#[cfg(test)]
pub(crate) mod tests;

/// The values an array callback method passes its callback: the element, its index and the
/// array, after a reducer's accumulator (`step_rules::takes_accumulator`).
fn array_callback_argument_count(native: Native) -> usize {
    const ELEMENT_INDEX_ARRAY: usize = 3;
    ELEMENT_INDEX_ARRAY + usize::from(step_rules::takes_accumulator(Kind::of(native)))
}

pub(crate) use quench_runtime::vm::engine_types::LaneArrayStep;



/// An array callback method's callback and `thisArg` among the values it was passed; a
/// reducer's second value is its initial accumulator, and its callback's `thisArg` is
/// `undefined`.
#[inline(always)]
fn array_callback_arguments(native: Native, args: &[Value]) -> Option<(Value, Value)> {
    let this_arg = if step_rules::takes_accumulator(Kind::of(native)) { None } else { args.get(1).copied() };
    Some((*args.first()?, this_arg.unwrap_or(Value::UNDEFINED)))
}

/// The activation of an array callback method `native`'s `callback` called with `this_arg`
/// from the call record `ip`, passed its values (`array_callback_argument_count`),
/// completing as `completion` says: its shape and entry (`PreparedShape`). `None` for a
/// callback no lane call activates.
#[inline(always)]
fn array_callback_activation_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    native: Native,
    callback: Value,
    this_arg: Value,
    completion: Completion,
) -> Option<(PreparedShape, PreparedEntry)> {
    let mut resolved = None;
    let target = callback_target_in_lane(cx, ip, callback, &mut resolved)?;
    let this = call_this_in_lane(unsafe { vm(cx) }, target, this_arg)?;
    // SAFETY: the running lane's context, which nothing else accesses.
    let mut shape = PreparedShape::of(unsafe { &*cx }, ip, target, array_callback_argument_count(native))?;
    if let ActivationKind::Script(script) = shape.activation.kind_mut() {
        script.completion = completion;
    }
    // A callback that keeps nothing may be pushed as a plain activation returning through the
    // array steps' trampoline record (`array_step_returned`), not a native's callback's: its
    // header is the shape's with that call record (`push_array_step_in_lane`). Its record, which
    // a keeping callback and a lane body's iteration push, has the step's completion.
    if let Some(header) = &mut shape.header {
        header.plain.call = unsafe { vm(cx) }.lane_stack.callback_return.step_call();
    }
    let entry = shape.entry(target, this);
    Some((shape, entry))
}

/// `script_target_in_in_lane` of an array callback method's `callback` at its call record `ip`,
/// whose `CallOperand::memo` slot, which no native's call reads, links the callback's cache
/// entry as a script call's `call_link` links its callee's: the link, tested first, skips the
/// set's hash. The record's `call_link` stays its own callee's, which a lane body reads
/// (`site_targets_in_lane`).
#[inline(always)]
fn callback_target_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    callback: Value,
    resolved: &mut Option<ScriptCallTarget>,
) -> Option<&ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    // SAFETY: a call record's operand is `call`.
    let link = unsafe { (*ip).operand.call.memo } as usize;
    if vm.script_call_holds(link, callback) && !constructing_in_lane(vm, callback) {
        // SAFETY: `script_call_holds` found the entry within the cache, which nothing
        // resolves into before the caller reads it.
        let target = unsafe { &*std::ptr::from_ref(vm.lane_stack.script_targets.entries.get_unchecked(link)) };
        return admits_activation_in_lane(target).then_some(target);
    }
    let target = script_target_in_in_lane(cx, callback, resolved)?;
    if let Some(index) = vm.script_call_index(callback) {
        // SAFETY: as `requicken`: the record's operand is the call's, written in place.
        unsafe { (*ip.cast_mut()).operand.call.memo = index as u32 };
    }
    Some(target)
}

/// Whether a target of the running program activates in the lane from a path besides a
/// plain call (`script_target_in_in_lane`): not a frame, an environment or a settling one.
#[inline(always)]
pub(crate) fn admits_activation_in_lane(target: &ScriptCallTarget) -> bool {
    !target.entry.needs_frame_path()
}

/// `forEach` at the lane `Call` record `ip`, whose result nothing reads
/// (`call_result_dead_in_lane`), over a dense array `receiver` whose storage holds one element (the
/// caller tested it), with `args`: where the element is present, the callback's one call with
/// it, its index and the array, a plain activation (`Completion::Plain`) whose value the dead
/// register takes. The method's own value, `undefined`, is never read, and the length it read
/// bounds its loop to that call. `None` leaves the call to `begin_array_iteration_in_lane`.
#[inline(never)]
pub(crate) fn call_for_each_once_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    receiver: Value,
    args: &[Value],
) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    // The callback's target is found through the admission word (`callback_target_in_lane`).
    if vm.with_scope_entered || vm.own_array_length(receiver) != Some(1) {
        return None;
    }
    let element = lane_dense_element(vm, receiver, 0)?;
    let (callback, this_arg) = array_callback_arguments(Native::ArrayForEach, args)?;
    let (shape, entry) = array_callback_activation_in_lane(cx, ip, Native::ArrayForEach, callback, this_arg, Completion::Plain)?;
    let window = admit_prepared_in_lane(cx, &shape.activation, shape.room)?;
    // SAFETY: the admission pushed the activation over the window.
    let start = unsafe { enter_prepared_values_in_lane(cx, &entry, [element, Value::length(0), receiver], window) };
    // SAFETY: the entry made the callback's registers the context's.
    Some((unsafe { (*cx).registers }, start))
}

/// The call of an array callback method `native` at the lane `Call` record `ip` over a dense
/// array `receiver` with `args`, run in the lane as the bytecode JSC's builtin runs: each
/// element's callback is an activation of the caller's lane (`Completion::ArrayStep`) whose
/// return steps the loop and enters the activation again for the next element
/// (`step_array_iteration_in_place_in_lane`), with no native frame and no nested run. `None`
/// leaves the call to the native: another receiver, a callback no lane call activates, or a
/// result the species makes.
#[inline(never)]
pub(crate) fn begin_array_iteration_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    native: Native,
    receiver: Value,
    args: &[Value],
) -> Option<Resumed> {
    let (mut iteration, shape, entry) = array_iteration_setup_in_lane(cx, ip, native, receiver, args)?;
    let vm = unsafe { vm(cx) };
    let Some(index) = iteration.advance() else {
        // SAFETY: `ip` is the call record of the running function's view.
        return Some(native_complete_in_lane(cx, ip, unsafe { &*ip }, Some(iteration.completion())));
    };
    if let Some(element) = lane_dense_element(vm, receiver, index) {
        // SAFETY: the running lane's context.
        let caller_base = unsafe { (*cx).base };
        let step = LaneArrayStep { iteration, element, index, entry, to_body: false, header: shape.header, caller_base };
        if let Some(start) = push_array_step_in_lane(cx, &shape, step) {
            // SAFETY: the entry made the callback's registers the context's.
            return Some(Resumed::At(unsafe { (*cx).registers }, start, Carried::script(cx)));
        }
    }
    iteration.visited -= 1;
    Some(resume_array_iteration_in_lane(cx, ip, iteration, false))
}

/// `begin_array_iteration_in_lane`'s setup: the iteration, with a reducer's accumulator, and
/// its callback's activation. `None` where the lane leaves the call to the native.
#[inline(always)]
fn array_iteration_setup_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    native: Native,
    receiver: Value,
    args: &[Value],
) -> Option<(quench_runtime::vm::array_indexed::ArrayIteration, PreparedShape, PreparedEntry)> {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } || vm.with_scope_entered {
        return None;
    }
    let length = array_object_in_lane(vm, receiver)?.elements().len();
    if vm.own_array_length(receiver) != Some(length) {
        return None;
    }
    let (callback, this_arg) = array_callback_arguments(native, args)?;
    let (shape, entry) = array_callback_activation_in_lane(cx, ip, native, callback, this_arg, Completion::ArrayStep)?;
    let mut iteration = vm.fresh_array_iteration(native, receiver, callback, this_arg, length)?;
    note_allocation(vm);
    // A reducer without an initial value starts from its first element, which a dense
    // array holds present; anything else leaves the call to the native.
    if step_rules::takes_accumulator(Kind::of(native)) {
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
pub(crate) fn array_object_in_lane(vm: &Vm, receiver: Value) -> Option<&quench_runtime::tier_surface::ArrayObject> {
    match vm.heap_reads().record_array(receiver) {
        Some(object) => Some(object),
        None => vm.heap_reads().array(receiver),
    }
}

/// The present element at `index` of `receiver`'s dense storage: an own data property,
/// which HasProperty finds and Get reads observing nothing.
#[inline(always)]
fn lane_dense_element(vm: &Vm, receiver: Value, index: usize) -> Option<Value> {
    step_rules::present(array_object_in_lane(vm, receiver)?.elements().get(index)?)
}

/// The iteration `iteration` of the call at `ip`, whose caller runs, past its last element:
/// the call completes with the method's value; else the rest of its loop on the native's
/// path from the caller's frame (`Vm::array_iteration_run`), which reads holes and anything
/// else generically.
#[inline(always)]
fn resume_array_iteration_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    iteration: quench_runtime::vm::array_indexed::ArrayIteration,
    to_body: bool,
) -> Resumed {
    if step_rules::exhausted(iteration.visited, iteration.length) {
        return array_complete_in_lane(cx, ip, to_body, Some(iteration.completion()));
    }
    run_array_iteration_from_frame_in_lane(cx, ip, iteration, to_body)
}

/// The call at `ip` of an array callback method completed with `value` (`None`: it raised):
/// the lane continues at the next record, or, for a lane body's call (`to_body`), the nested
/// run the body made ends with the value in the call's register, as a body's callee's return
/// ends it (`body_calls::ends_run`).
#[inline(always)]
fn array_complete_in_lane(cx: *mut LaneContext, ip: Ip, to_body: bool, value: Option<Value>) -> Resumed {
    // SAFETY: `ip` is the call record of the running function's view.
    let i = unsafe { &*ip };
    if !to_body {
        return native_complete_in_lane(cx, ip, i, value);
    }
    let Some(value) = value else { return Resumed::Exit(exit(ip)) };
    unsafe { (*cx).registers }.set(i.a, value);
    unsafe { vm(cx) }.lane_completion.set(LaneCompletion::NativeReturned(value));
    Resumed::Exit(exit(ip))
}

/// `resume_array_iteration_in_lane` of an iteration with elements left: the native's loop.
#[inline(never)]
fn run_array_iteration_from_frame_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    mut iteration: quench_runtime::vm::array_indexed::ArrayIteration,
    to_body: bool,
) -> Resumed {
    let value = call_from_frame_in_lane(cx, ip, |vm, code| vm.array_iteration_run(code, &mut iteration));
    // SAFETY: `ip` is the call record of the running function's view.
    let i = unsafe { &*ip };
    if !to_body {
        return native_complete_in_lane(cx, ip, i, value);
    }
    // The loop made the body's activations frames, which the general path continues after
    // the call.
    let Some(value) = value else { return Resumed::Exit(exit(ip)) };
    unsafe { (*cx).registers }.set(i.a, value);
    Resumed::Exit(exit(following(ip)))
}

/// `begin_array_iteration_in_lane` for a lane body's call of the array callback method `native` at
/// `ip` (`native_calls::body_native_out_in_lane`): the first element's callback is an activation
/// above the body, run nested (`body_calls::finish_array_iteration`) until the iteration
/// completes, which writes the call's register, so the body continues after the call. `None`
/// leaves the call to the native's call out: what the lane iteration declines, an empty array,
/// a hole at the first element, or no native stack for a nested run.
pub(crate) fn body_array_iteration_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    native: Native,
    receiver: Value,
    args: &[Value],
) -> Option<u64> {
    let _stack = quench_runtime::stack::StackGuard::enter().ok()?;
    let (mut iteration, shape, entry) = array_iteration_setup_in_lane(cx, ip, native, receiver, args)?;
    let vm = unsafe { vm(cx) };
    let index = iteration.advance()?;
    let element = lane_dense_element(vm, receiver, index)?;
    // A body's iteration keeps its callback off the trampoline: the nested run's boundaries read
    // its completion, which its side record holds (`Completion::ArrayStep`, one-frame-one-call.md F4 (3)).
    let caller_base = unsafe { (*cx).base };
    let window = admit_prepared_in_lane(cx, &shape.activation, shape.room)?;
    // SAFETY: the admission pushed the activation over the window.
    let start = unsafe { enter_array_step_in_lane::<false>(cx, &entry, &iteration, element, index, window) };
    vm.lane_stack.steps.push(LaneArrayStep { iteration, element, index, entry, to_body: true, header: None, caller_base });
    Some(body_calls::finish_array_iteration(cx, start))
}

/// The return of an array callback method's callback `activation` with `mapped`, its
/// caller restored (`restore_caller_in_lane`): the step of the innermost lane iteration, then
/// the rest of its loop (`finish_array_step_in_lane`).
pub(crate) fn step_array_iteration_in_lane(cx: *mut LaneContext, frame: &ActivationView, mapped: Value) -> Resumed {
    let applied = apply_array_step_in_lane(cx, mapped);
    finish_array_step_in_lane(cx, frame.call(), applied)
}

/// The step of the innermost lane iteration whose callback returned `mapped`
/// (`staged::array_iteration::apply`): the method's value where the step ends it.
#[inline(always)]
fn apply_array_step_in_lane(cx: *mut LaneContext, mapped: Value) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let step: *mut LaneArrayStep = vm.lane_stack.steps.last_mut().expect("each array step activation has its entry");
    // SAFETY: the entry stays in place while the step, which reads no lane stack, runs.
    lane_apply_step_value(vm, unsafe { &mut *step }, mapped)
}

/// `apply_array_step_in_lane` of the innermost entry `step` already holds, so the in-place step
/// does not look the entry up again: the step applied to the callback's `mapped`.
#[inline(always)]
fn lane_apply_step_value(vm: &mut Vm, step: &mut LaneArrayStep, mapped: Value) -> Option<Value> {
    let effect = vm.iteration_effect(&mut step.iteration, step.index, step.element, mapped);
    let applied = vm.apply_fresh_effect(&mut step.iteration, effect);
    note_allocation(vm);
    applied
}

/// `step_array_iteration_in_lane` of the innermost activation, a callback that returned
/// `mapped`: where the step goes on to a present dense element and no collection is due,
/// the activation is entered again for it in place, as the lane's own loop of calls would
/// make its next one (`enter_prepared_in_lane`); otherwise the activation retires to its
/// caller first.
///
/// The step runs inside the returning handler: a `forEach` applies nothing, and the next
/// element is read from the receiver's dense storage by its index alone.
#[inline(always)]
pub(crate) fn step_array_iteration_in_place_in_lane(cx: *mut LaneContext, mapped: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let step: *mut LaneArrayStep = vm.lane_stack.steps.last_mut().expect("each array step activation has its entry");
    // SAFETY: the entry stays in place while the step, which reads no lane stack, runs and
    // the callback's window is filled again.
    let step = unsafe { &mut *step };
    let applied = if step_rules::applies_results(Kind::of(step.iteration.native)) {
        lane_apply_step_value(vm, step, mapped)
    } else {
        None
    };
    let depth = vm.lane_stack.activation_count() - 1;
    // The re-entry's admission is the callback family's depth row
    // (`staged::call_callback::admit_depth`); the window is the one its return left.
    if applied.is_none()
        && quench_runtime::staged::call_callback::concrete_below(depth as u64, vm.lane_stack.activation_limit.get() as u64)
        && let Some(index) = step.iteration.advance()
    {
        let receiver = step.iteration.receiver;
        if let Some(element) = lane_dense_element(vm, receiver, index) {
            step.element = element;
            step.index = index;
            // The callback's completed loops close nothing more.
            vm.lane_stack.drop_owned_from(depth);
            // The innermost's window read where it is (no spill, one-frame-one-call.md F4): a
            // plain callback's header or its record.
            let window = vm.lane_stack.top_frame().expect("an array step's activation runs").window_ptr();
            // SAFETY: the window the activation held, which its return left in place.
            let start = unsafe { enter_array_step_in_lane::<true>(cx, &step.entry, &step.iteration, element, index, window) };
            return Some(Resumed::At(unsafe { (*cx).registers }, start, Carried::script(cx)));
        }
        step.iteration.visited -= 1;
    }
    Some(retire_array_step_in_lane(cx, applied))
}

/// `step_array_iteration_in_place_in_lane` past the in-place step: the callback's activation
/// retires to its caller, then the iteration finishes (`finish_array_step_in_lane`).
#[inline(never)]
fn retire_array_step_in_lane(cx: *mut LaneContext, applied: Option<Value>) -> Resumed {
    let vm = unsafe { vm(cx) };
    let frame = vm.lane_stack.pop_activation().expect("an array step's activation runs");
    restore_caller_in_lane(cx, &frame);
    finish_array_step_in_lane(cx, frame.call(), applied)
}

/// The iteration of the call record `call` (its callback's activation's), its caller
/// restored, after a step that `applied`: the iteration retires, and the call completes with
/// the method's value where the step ended it, else goes on as `resume_array_iteration_in_lane`
/// does.
#[inline(always)]
fn finish_array_step_in_lane(cx: *mut LaneContext, call: Ip, applied: Option<Value>) -> Resumed {
    let vm = unsafe { vm(cx) };
    let depth = vm.lane_stack.steps.len() - 1;
    let LaneArrayStep { iteration, to_body, .. } = vm.lane_stack.steps[depth];
    vm.lane_stack.steps.truncate(depth);
    match applied {
        None => resume_array_iteration_in_lane(cx, call, iteration, to_body),
        value => array_complete_in_lane(cx, call, to_body, value),
    }
}

/// `begin_array_iteration_in_lane`'s push of `step`'s first callback: its plain header where the
/// shape has one (`LaneStack::push_step_header`), which returns through the array steps'
/// trampoline (`array_step_returned`), else its header with a side record (`admit_prepared_in_lane`); then the
/// callback entered for the step's element. The entry is pushed first, so a spill of the
/// header finds its step (`LaneStack::plain_frame`). `None`, which changes nothing, where the
/// lane stack lacks room.
#[inline(always)]
fn push_array_step_in_lane(cx: *mut LaneContext, shape: &PreparedShape, mut step: LaneArrayStep) -> Option<Ip> {
    let vm = unsafe { vm(cx) };
    let index = vm.lane_stack.steps.len();
    if let Some(header) = &mut step.header {
        header.plain.caller_base = index as u64;
    }
    vm.lane_stack.steps.push(step);
    let admitted = match &step.header {
        Some(header) => admit_step_header(cx, header, shape.room),
        None => admit_prepared_in_lane(cx, &shape.activation, shape.room),
    };
    let Some(window) = admitted else {
        vm.lane_stack.steps.pop();
        return None;
    };
    // SAFETY: the admission pushed the activation over the window.
    Some(unsafe { enter_array_step_in_lane::<false>(cx, &step.entry, &step.iteration, step.element, step.index, window) })
}

/// `admit_prepared_in_lane` of an array step's callback header (`LaneStack::push_step_header`):
/// the callback family's admission row, then the header over a window at the values' top.
#[inline(always)]
fn admit_step_header(cx: *mut LaneContext, header: &CallbackHeader, room: usize) -> Option<*mut Value> {
    let stack = &mut unsafe { vm(cx) }.lane_stack;
    if !admits_callback(stack, room) {
        return None;
    }
    let at = stack.values.top();
    // SAFETY: admitted above; the window starts at the values' top; the step's entry is pushed.
    unsafe { stack.push_step_header(header, at) };
    Some(at)
}

script_handler! {
    /// `steps[1]` of the return trampoline (`CallbackTrampoline`): an in-lane array step's
    /// callback, a plain activation, returned. Its plain return popped its header, released its
    /// window and wrote its value to the trampoline's result register; the innermost step takes
    /// it (`array_step_returned_in_lane`).
    pub(crate) fn array_step_returned<>(cx, r, ip, i, view) {
        resume!(cx, array_step_returned_in_lane(cx, r.get(i.a)))
    }
}

/// The innermost array step's plain callback returned `mapped` (`array_step_returned`): the
/// step applies it (`lane_apply_step_value`: a map's element stored, a filter's kept, a reducer's
/// accumulator, a find's answer; a `forEach` applies nothing), then the callback's header is
/// pushed again for the next element without leaving the handler (`step_header_again_in_lane`),
/// or the iteration finishes in the callback's caller (`finish_step_header_in_lane`). As
/// `step_array_iteration_in_place_in_lane` steps a callback that is a record, the same steps.
#[inline(always)]
fn array_step_returned_in_lane(cx: *mut LaneContext, mapped: Value) -> Resumed {
    let vm = unsafe { vm(cx) };
    let step: *mut LaneArrayStep = vm.lane_stack.steps.last_mut().expect("each array step activation has its entry");
    // SAFETY: the entry stays in place while the step, which pushes no entry, runs and the
    // callback's window is filled again.
    let step = unsafe { &mut *step };
    let header = step.header.expect("an array step's plain callback has its header");
    // The pop named the trampoline's result window as the innermost plain one; it is the
    // callback's caller's where plain activations remain below it.
    vm.lane_stack.plain.registers.set(header.caller_registers.0);
    let applied = if step_rules::applies_results(Kind::of(step.iteration.native)) {
        lane_apply_step_value(vm, step, mapped)
    } else {
        None
    };
    if applied.is_none()
        && let Some(start) = step_header_again_in_lane(cx, step, &header)
    {
        // SAFETY: the entry made the callback's registers the context's.
        return Resumed::At(unsafe { (*cx).registers }, start, Carried::script(cx));
    }
    finish_step_header_in_lane(cx, &header, applied)
}

/// `array_step_returned_in_lane` going on to `step`'s next element: where the activation limit
/// admits the callback again (the callback family's depth row, which a due collection refuses)
/// and the next element is a present dense one, `header` is pushed again over the window the
/// return released, at the values' top, which holds the room its first admission found (no
/// window moves while activations exist), and the callback entered for the element. `None`
/// leaves the step to finish with its iteration as it was.
#[inline(always)]
fn step_header_again_in_lane(cx: *mut LaneContext, step: &mut LaneArrayStep, header: &CallbackHeader) -> Option<Ip> {
    let vm = unsafe { vm(cx) };
    let depth = vm.lane_stack.activation_count();
    if !quench_runtime::staged::call_callback::concrete_below(depth as u64, vm.lane_stack.activation_limit.get() as u64) {
        return None;
    }
    let index = step.iteration.advance()?;
    let Some(element) = lane_dense_element(vm, step.iteration.receiver, index) else {
        step.iteration.visited -= 1;
        return None;
    };
    step.element = element;
    step.index = index;
    // The callback's completed loops close nothing more.
    vm.lane_stack.drop_owned_from(depth);
    let window = vm.lane_stack.values.top();
    // SAFETY: admitted above, over the window the callback's return released; its entry stays.
    unsafe { vm.lane_stack.push_step_header(header, window) };
    // SAFETY: the header was pushed over the window.
    Some(unsafe { enter_array_step_in_lane::<false>(cx, &step.entry, &step.iteration, element, index, window) })
}

/// `array_step_returned_in_lane` past the step's last callback, which `applied`: the callback's
/// caller's registers and records the context's again (`header`'s call record and registers,
/// its step's base), what the callback owned dropped, as a record's retirement
/// (`retire_array_step_in_lane`) restores them, then the iteration finishes
/// (`finish_array_step_in_lane`).
#[inline(never)]
fn finish_step_header_in_lane(cx: *mut LaneContext, header: &CallbackHeader, applied: Option<Value>) -> Resumed {
    let vm = unsafe { vm(cx) };
    let caller_base = vm.lane_stack.steps.last().expect("each array step activation has its entry").caller_base;
    // SAFETY: the running lane's context, which nothing else accesses.
    unsafe {
        (*cx).registers = header.caller_registers;
        (*cx).base = caller_base;
    }
    vm.lane_stack.drop_owned_from(vm.lane_stack.activation_count());
    finish_array_step_in_lane(cx, header.call, applied)
}

/// The callback activation of `iteration`'s step passed `element` at `index`: entered
/// (`enter_prepared_values_in_lane`), or, `AGAIN`, entered again in the window its return left
/// (`reenter_prepared_values_in_lane`). The callback is passed the element, its index and the
/// array, after a reducer's accumulator.
///
/// # Safety
/// As the entry's.
#[inline(always)]
unsafe fn enter_array_step_in_lane<const AGAIN: bool>(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    iteration: &quench_runtime::vm::array_indexed::ArrayIteration,
    element: Value,
    index: usize,
    window: *mut Value,
) -> Ip {
    let (index, receiver) = (Value::length(index), iteration.receiver);
    unsafe {
        if step_rules::takes_accumulator(Kind::of(iteration.native)) {
            let values = [iteration.result, element, index, receiver];
            if AGAIN { reenter_prepared_values_in_lane(cx, entry, values, window) } else { enter_prepared_values_in_lane(cx, entry, values, window) }
        } else {
            let values = [element, index, receiver];
            if AGAIN { reenter_prepared_values_in_lane(cx, entry, values, window) } else { enter_prepared_values_in_lane(cx, entry, values, window) }
        }
    }
}
