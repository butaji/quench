//! `AbortSignal` and `AbortController` (`lib/internal/abort_controller.js`).
//!
//! A signal is an EventTarget with one private slot: whether it aborted and why, the
//! composite signals that follow it (`AbortSignal.any`), and its `onabort` handler. Aborting
//! marks the signal and every dependent aborted first, then dispatches each one's trusted
//! `abort` event in that order.
use super::event_target_shared_vm::{
    api, argument, attach_slot, custom_inspect, flag, get, install_members, install_tag_and_inspect,
    is_undefined, put, put_flag, receiver_slot, require_new, slot, Member, Shallow,
};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

pub(super) const SIGNAL_SLOT: &str = "signalSlot";
pub(super) const CONTROLLER_SLOT: &str = "controllerSlot";
const ABORT_EVENT: &str = "abort";

/// A signal slot's fields.
mod field {
    pub(super) const ABORTED: &str = "aborted";
    pub(super) const REASON: &str = "reason";
    pub(super) const DEPENDENTS: &str = "dependents";
    pub(super) const SOURCES: &str = "sources";
    pub(super) const COMPOSITE: &str = "composite";
    pub(super) const HANDLER: &str = "handler";
}

/// `AbortSignal.timeout`'s `validateUint32` bound.
const MAX_TIMEOUT_DELAY: f64 = u32::MAX as f64;

/// The reasons the host aborts with: (message, DOMException name).
const ABORTED: (&str, &str) = ("This operation was aborted", "AbortError");
const TIMED_OUT: (&str, &str) = ("The operation was aborted due to timeout", "TimeoutError");

/// The private-field error of a controller method on a foreign receiver.
const FOREIGN_CONTROLLER: &str = "Cannot read private member #signal from an object whose class did not declare it";

const SIGNAL_PROTOTYPE_ENUMERABLE_HEAD: [Member; 1] = [Member::Accessor("aborted", "abortSignalAborted", None, 0.0)];
const SIGNAL_PROTOTYPE_HIDDEN: [Member; 2] = [
    Member::Accessor("reason", "abortSignalReason", None, 0.0),
    Member::Method("throwIfAborted", "abortSignalThrowIfAborted", 0.0),
];
const SIGNAL_PROTOTYPE_ENUMERABLE_TAIL: [Member; 1] =
    [Member::Accessor("onabort", "abortSignalGetOnabort", Some("abortSignalSetOnabort"), 0.0)];
const SIGNAL_STATICS: [Member; 3] = [
    Member::Method("abort", "abortSignalStaticAbort", 0.0),
    Member::Method("timeout", "abortSignalStaticTimeout", 0.0),
    Member::Method("any", "abortSignalStaticAny", 0.0),
];
const CONTROLLER_MEMBERS: [Member; 2] = [
    Member::Accessor("signal", "abortControllerSignal", None, 0.0),
    Member::Method("abort", "abortControllerAbort", 0.0),
];

/// Builds both classes and records them in the event group's data.
pub(crate) fn install(c: &mut Context<'_>, data: RootId) -> Result<(), RootedError> {
    let operation = crate::host::shared_vm::operation;
    let target = get(c, data, "eventTargetCtor")?;
    let target_prototype = get(c, target, "prototype")?;
    let signal = c.host_constructor_named(operation("abortSignalConstructor"), "AbortSignal")?;
    c.set_prototype_rooted(signal, target)?;
    let prototype = get(c, signal, "prototype")?;
    c.set_prototype_rooted(prototype, target_prototype)?;
    install_members(c, prototype, &SIGNAL_PROTOTYPE_ENUMERABLE_HEAD, true)?;
    install_members(c, prototype, &SIGNAL_PROTOTYPE_HIDDEN, false)?;
    install_members(c, prototype, &SIGNAL_PROTOTYPE_ENUMERABLE_TAIL, true)?;
    install_members(c, signal, &SIGNAL_STATICS, false)?;
    install_tag_and_inspect(c, prototype, "AbortSignal", Some("abortSignalInspect"))?;
    put(c, data, "abortSignalCtor", signal)?;
    let controller = c.host_constructor_named(operation("abortControllerConstructor"), "AbortController")?;
    let prototype = get(c, controller, "prototype")?;
    install_members(c, prototype, &CONTROLLER_MEMBERS, true)?;
    install_tag_and_inspect(c, prototype, "AbortController", Some("abortControllerInspect"))?;
    put(c, data, "abortControllerCtor", controller)?;
    let abort = c.host_function_named(operation("abortSignalAbort"), "abortSignal")?;
    put(c, data, "abortSignal", abort)?;
    Ok(())
}

/// Whether `value` is a genuine AbortSignal.
pub(crate) fn is_abort_signal(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    Ok(slot(c, value, SIGNAL_SLOT)?.is_some())
}

/// `signal.aborted`, read from a genuine signal's slot or, for a duck-typed one, its property.
pub(crate) fn aborted(c: &mut Context<'_>, signal: RootId) -> Result<bool, RootedError> {
    match slot(c, signal, SIGNAL_SLOT)? {
        Some(state) => flag(c, state, field::ABORTED),
        None => flag(c, signal, field::ABORTED),
    }
}

/// A new signal, aborted for `reason` when `reason` is given.
fn create_signal(c: &mut Context<'_>, reason: Option<RootId>, composite: bool) -> Result<RootId, RootedError> {
    let data = api(c)?;
    let constructor = get(c, data, "abortSignalCtor")?;
    let prototype = get(c, constructor, "prototype")?;
    let signal = c.object_rooted()?;
    c.set_prototype_rooted(signal, prototype)?;
    let maximum = c.number(super::event_limits_shared_vm::ABORT_SIGNAL_MAX_LISTENERS);
    super::event_target_shared_vm::initialize_target(c, signal, maximum)?;
    let state = c.null_object_rooted()?;
    put_flag(c, state, field::ABORTED, reason.is_some())?;
    let reason = reason.unwrap_or_else(|| c.undefined());
    put(c, state, field::REASON, reason)?;
    for name in [field::DEPENDENTS, field::SOURCES] {
        let list = c.array_rooted(&[])?;
        put(c, state, name, list)?;
    }
    put_flag(c, state, field::COMPOSITE, composite)?;
    attach_slot(c, signal, SIGNAL_SLOT, state)?;
    Ok(signal)
}

fn signal_slot(c: &mut Context<'_>, receiver: RootId) -> Result<RootId, RootedError> {
    receiver_slot(c, receiver, SIGNAL_SLOT, "AbortSignal")
}

fn dom_exception(c: &mut Context<'_>, (message, name): (&str, &str)) -> Result<RootId, RootedError> {
    let message = c.string_rooted(message);
    let name = c.string_rooted(name);
    super::dom_exception_shared_vm::create(c, message, name)
}

/// `reason` unless undefined, else a new `AbortError` DOMException.
fn abort_reason(c: &mut Context<'_>, reason: RootId) -> Result<RootId, RootedError> {
    if is_undefined(c, reason) {
        dom_exception(c, ABORTED)
    } else {
        Ok(reason)
    }
}

fn values(c: &mut Context<'_>, state: RootId, name: &str) -> Result<Vec<RootId>, RootedError> {
    let list = get(c, state, name)?;
    c.array_like_values_rooted(list)
}

fn append(c: &mut Context<'_>, state: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let list = get(c, state, name)?;
    let push = get(c, list, "push")?;
    c.call_rooted(push, list, &[value])?;
    Ok(())
}

/// Node's `abortSignal(signal, reason)`.
pub(crate) fn abort(c: &mut Context<'_>, signal: RootId, reason: RootId) -> Result<(), RootedError> {
    let state = signal_slot(c, signal)?;
    if flag(c, state, field::ABORTED)? {
        return Ok(());
    }
    let mut order = vec![(signal, state)];
    let mut index = 0;
    while index < order.len() {
        let (_, state) = order[index];
        put_flag(c, state, field::ABORTED, true)?;
        put(c, state, field::REASON, reason)?;
        for dependent in values(c, state, field::DEPENDENTS)? {
            let dependent_state = signal_slot(c, dependent)?;
            if !flag(c, dependent_state, field::ABORTED)? && !order.iter().any(|&(_, seen)| seen == dependent_state) {
                order.push((dependent, dependent_state));
            }
        }
        index += 1;
    }
    for (signal, _) in order {
        let event = super::event_target_shared_vm::trusted_event(c, ABORT_EVENT)?;
        let event_state = receiver_slot(c, event, super::event_target_shared_vm::EVENT_SLOT, "Event")?;
        super::event_target_shared_vm::dispatch(c, signal, event, event_state)?;
    }
    Ok(())
}

/// `abortSignal(signal, reason)`, the internal operation of `internal/abort_controller`.
pub(crate) fn abort_operation(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let signal = argument(c, args, 0);
    let reason = argument(c, args, 1);
    abort(c, signal, reason)?;
    Ok(c.undefined())
}

/// `new AbortSignal()`: only the host makes signals.
pub(crate) fn signal_constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    require_new(c, "AbortSignal")?;
    let error = c.type_error_rooted("Illegal constructor")?;
    super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_ILLEGAL_CONSTRUCTOR")?;
    Err(c.throw(error))
}

pub(crate) fn signal_aborted(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = signal_slot(c, receiver)?;
    get(c, state, field::ABORTED)
}

pub(crate) fn signal_reason(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = signal_slot(c, receiver)?;
    get(c, state, field::REASON)
}

pub(crate) fn signal_throw_if_aborted(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = signal_slot(c, receiver)?;
    if flag(c, state, field::ABORTED)? {
        let reason = get(c, state, field::REASON)?;
        return Err(c.throw(reason));
    }
    Ok(c.undefined())
}

/// `get onabort`: the handler, or null.
pub(crate) fn signal_get_onabort(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = signal_slot(c, receiver)?;
    super::event_target_shared_vm::event_handler(c, state, field::HANDLER)
}

pub(crate) fn signal_set_onabort(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = signal_slot(c, receiver)?;
    let value = argument(c, args, 0);
    super::event_target_shared_vm::set_event_handler(c, receiver, state, field::HANDLER, ABORT_EVENT, value)?;
    Ok(c.undefined())
}

/// `AbortSignal.abort(reason)`.
pub(crate) fn static_abort(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let reason = argument(c, args, 0);
    let reason = abort_reason(c, reason)?;
    create_signal(c, Some(reason), false)
}

/// `AbortSignal.timeout(delay)`: a signal an unreferenced timer aborts with a `TimeoutError`.
pub(crate) fn static_timeout(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let delay = argument(c, args, 0);
    let maximum = c.number(MAX_TIMEOUT_DELAY);
    super::util_shared_vm::validate_integer(c, delay, "delay", 0.0, maximum)?;
    let signal = create_signal(c, None, false)?;
    let fire = c.host_function_with_data(crate::host::shared_vm::operation("abortSignalTimeoutFire"), signal)?;
    super::timers_shared_vm::schedule_timer(c, &[fire, delay], false, false)?;
    Ok(signal)
}

pub(crate) fn timeout_fire(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let signal = c.host_function_data()?;
    let reason = dom_exception(c, TIMED_OUT)?;
    abort(c, signal, reason)?;
    Ok(c.undefined())
}

/// The `sequence<AbortSignal>` conversion's failure.
fn not_a_sequence(c: &mut Context<'_>) -> RootedError {
    match c.type_error_rooted("Value can not be converted to sequence.") {
        Ok(error) => match super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_ARG_TYPE") {
            Ok(()) => c.throw(error),
            Err(error) => error,
        },
        Err(error) => error,
    }
}

/// `AbortSignal.any(signals)`: a composite signal following the non-composite sources of
/// `signals`, aborted at once by the first already aborted one.
pub(crate) fn static_any(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let signals = argument(c, args, 0);
    if !super::event_target_shared_vm::is_object_like(c, signals)? {
        return Err(not_a_sequence(c));
    }
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let iterator_key = get(c, symbol, "iterator")?;
    let method = c.get_property_rooted(signals, iterator_key)?;
    if !c.is_callable_rooted(method)? {
        return Err(not_a_sequence(c));
    }
    let iterator = c.iterator_rooted(signals)?;
    let mut sources = Vec::new();
    while let Some(signal) = c.iterator_step_rooted(&iterator)? {
        sources.push(signal);
    }
    for (index, &signal) in sources.iter().enumerate() {
        let name = format!("signals[{index}]");
        if is_undefined(c, signal) {
            return Err(super::util_shared_vm::argument_type_error(c, signal, &name, "AbortSignal")?);
        }
        super::util_shared_vm::validate_abort_signal(c, signal, &name)?;
    }
    let result = create_signal(c, None, true)?;
    let result_state = signal_slot(c, result)?;
    for signal in sources {
        if aborted(c, signal)? {
            let reason = get(c, signal, field::REASON)?;
            abort(c, result, reason)?;
            return Ok(result);
        }
        let Some(state) = slot(c, signal, SIGNAL_SLOT)? else { continue };
        let originals = if flag(c, state, field::COMPOSITE)? { values(c, state, field::SOURCES)? } else { vec![signal] };
        for source in originals {
            if values(c, result_state, field::SOURCES)?.iter().any(|&known| c.strict_equal_rooted(known, source).unwrap_or(false)) {
                continue;
            }
            append(c, result_state, field::SOURCES, source)?;
            let source_state = signal_slot(c, source)?;
            append(c, source_state, field::DEPENDENTS, result)?;
        }
    }
    Ok(result)
}

/// `[util.inspect.custom]`: `AbortSignal { aborted }`.
pub(crate) fn signal_inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = signal_slot(c, receiver)?;
    let body = c.object_rooted()?;
    let aborted = get(c, state, field::ABORTED)?;
    put(c, body, field::ABORTED, aborted)?;
    custom_inspect(c, receiver, body, args, Shallow::Receiver)
}

/// `new AbortController()`.
pub(crate) fn controller_constructor(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    require_new(c, "AbortController")?;
    let signal = create_signal(c, None, false)?;
    attach_slot(c, receiver, CONTROLLER_SLOT, signal)?;
    Ok(c.undefined())
}

fn controller_signal(c: &mut Context<'_>, receiver: RootId) -> Result<RootId, RootedError> {
    match slot(c, receiver, CONTROLLER_SLOT)? {
        Some(signal) => Ok(signal),
        None => {
            let error = c.type_error_rooted(FOREIGN_CONTROLLER)?;
            Err(c.throw(error))
        }
    }
}

pub(crate) fn controller_get_signal(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    controller_signal(c, receiver)
}

/// `abort(reason)`.
pub(crate) fn controller_abort(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let signal = controller_signal(c, receiver)?;
    let reason = argument(c, args, 0);
    let reason = abort_reason(c, reason)?;
    abort(c, signal, reason)?;
    Ok(c.undefined())
}

/// `[util.inspect.custom]`: `AbortController { signal }`.
pub(crate) fn controller_inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let signal = controller_signal(c, receiver)?;
    let body = c.object_rooted()?;
    put(c, body, "signal", signal)?;
    custom_inspect(c, receiver, body, args, Shallow::Receiver)
}
