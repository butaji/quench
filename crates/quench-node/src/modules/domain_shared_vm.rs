//! `node:domain` (lib/domain.js): domains as a stack of entered `Domain` emitters, the
//! active one mirrored in `process.domain`; emitters, async resources and native callbacks
//! made in a domain run inside it, and an uncaught error reaches the active domain's
//! `'error'` listeners through the uncaught-exception capture callback.
use super::event_constructor_shared_vm::get;
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

use crate::modules::keys::STATE;
const STATE_STACK: &str = "stack";
const STATE_EXPORTS: &str = "exports";
const STATE_SET_CAPTURE: &str = "setCapture";
const STATE_CAPTURE: &str = "capture";
const STATE_CLEAR: &str = "clear";
const STATE_UPDATE: &str = "update";
const STATE_CLASS: &str = "Domain";
const STATE_PAIRING: &str = "pairing";
const STATE_EMIT: &str = "emit";
const STATE_INIT: &str = "init";
const STATE_HOOK: &str = "hook";
const STATE_REQUIRE_STACK: &str = "requireStack";
const STATE_DEPRECATED: &str = "makeCallbackDeprecated";

const DOMAIN: &str = "domain";
const ACTIVE: &str = "active";
pub(crate) use crate::modules::keys::ERROR_EVENT;
const UNCAUGHT_EXCEPTION: &str = "uncaughtException";

/// `Domain.prototype`'s methods (Node assigns anonymous functions) with their operations.
const PROTOTYPE_METHODS: [(&str, &str); 8] = [
    ("_errorHandler", "domainErrorHandler"),
    ("enter", "domainEnter"),
    ("exit", "domainExit"),
    ("add", "domainAdd"),
    ("remove", "domainRemove"),
    ("run", "domainRun"),
    ("intercept", "domainIntercept"),
    ("bind", "domainBind"),
];
/// The async hook that carries the active domain into the callbacks of resources made in it.
const HOOK_CALLBACKS: [(&str, &str); 4] = [
    ("init", "domainHookInit"),
    ("before", "domainHookBefore"),
    ("after", "domainHookAfter"),
    ("destroy", "domainHookDestroy"),
];
const PROMISE_RESOURCE: &str = "PROMISE";
/// The `'-'.repeat(40)` rule between an error's stack and the `require('domain')` stack.
const STACK_RULE_WIDTH: usize = 40;
const MAKE_CALLBACK_DEPRECATION: (&str, &str) = (
    "Using a domain property in MakeCallback is deprecated. Use the async_context variant of MakeCallback or the AsyncResource class instead.",
    "DEP0097",
);

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_none_or(|value| value.is_null() || value.is_undefined())
}

fn put(c: &mut Context<'_>, target: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(target, key, value, true, true, true)?;
    Ok(())
}

fn hidden(c: &mut Context<'_>, target: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(target, key, value, true, false, true)?;
    Ok(())
}

fn assign(c: &mut Context<'_>, target: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(target, name, value)
}

fn call_method(c: &mut Context<'_>, target: RootId, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let method = get(c, target, name)?;
    c.call_rooted(method, target, args)
}

fn process(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process is not initialized"))
}

fn state(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut().shared_state().borrow().builtin_cache.get(STATE).copied().ok_or_else(|| RootedError::host("node:domain is not loaded"))
}

fn intrinsic(c: &mut Context<'_>, constructor: &str, member: &str) -> Result<RootId, RootedError> {
    c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let constructor = get(c, global, constructor)?;
        let prototype = get(c, constructor, "prototype")?;
        get(c, prototype, member)
    })
}

fn stack(c: &mut Context<'_>, state: RootId) -> Result<Vec<RootId>, RootedError> {
    let stack = get(c, state, STATE_STACK)?;
    c.array_like_values_rooted(stack)
}

/// Truncates the domain stack to `length` entries in place.
fn truncate_stack(c: &mut Context<'_>, state: RootId, length: usize) -> Result<(), RootedError> {
    let stack = get(c, state, STATE_STACK)?;
    let length = c.number(length as f64);
    assign(c, stack, "length", length)
}

/// `exports.active = process.domain = domain`.
fn set_active(c: &mut Context<'_>, state: RootId, domain: RootId) -> Result<(), RootedError> {
    let exports = get(c, state, STATE_EXPORTS)?;
    assign(c, exports, ACTIVE, domain)?;
    let process = process(c)?;
    assign(c, process, DOMAIN, domain)
}

/// Node's `updateExceptionCapture`: the capture callback is installed while some domain on
/// the stack has an `'error'` listener.
fn update_capture(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let set_capture = get(c, state, STATE_SET_CAPTURE)?;
    let process = process(c)?;
    let null = c.null();
    let mut listening = false;
    let event = c.string_rooted(ERROR_EVENT);
    for domain in stack(c, state)? {
        let count = call_method(c, domain, "listenerCount", &[event])?;
        if c.to_number(count)? != 0.0 {
            listening = true;
            break;
        }
    }
    c.call_rooted(set_capture, process, &[null])?;
    if listening {
        let capture = get(c, state, STATE_CAPTURE)?;
        c.call_rooted(set_capture, process, &[capture])?;
    }
    Ok(())
}

/// Node's `domainUncaughtExceptionClear`: no domain stays entered past an uncaught error.
fn clear(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    truncate_stack(c, state, 0)?;
    let null = c.null();
    set_active(c, state, null)?;
    update_capture(c, state)
}

fn enter(c: &mut Context<'_>, state: RootId, domain: RootId) -> Result<(), RootedError> {
    set_active(c, state, domain)?;
    let entries = stack(c, state)?;
    let stack = get(c, state, STATE_STACK)?;
    let index = c.number(entries.len() as f64);
    c.set_property_rooted(stack, index, domain, stack)?;
    update_capture(c, state)
}

/// Exits `domain` and every domain entered after it; a domain not on the stack is left be.
fn exit(c: &mut Context<'_>, state: RootId, domain: RootId) -> Result<(), RootedError> {
    let entries = stack(c, state)?;
    let mut index = None;
    for (position, entry) in entries.iter().enumerate().rev() {
        if c.strict_equal_rooted(*entry, domain)? {
            index = Some(position);
            break;
        }
    }
    let Some(index) = index else {
        return Ok(());
    };
    truncate_stack(c, state, index)?;
    let active = match index.checked_sub(1) {
        Some(top) => entries[top],
        None => c.undefined(),
    };
    set_active(c, state, active)?;
    update_capture(c, state)
}

fn is_domain(c: &mut Context<'_>, state: RootId, value: RootId) -> Result<bool, RootedError> {
    let class = get(c, state, STATE_CLASS)?;
    Ok(c.is_object_rooted(value)? && c.instance_of_rooted(value, class)?)
}

/// `node:domain`.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let process = process(c)?;
    let has_capture = call_method(c, process, "hasUncaughtExceptionCaptureCallback", &[])?;
    if c.truthy_rooted(has_capture)? {
        let error = c.error_rooted(
            "A callback was registered through process.setUncaughtExceptionCaptureCallback(), which is mutually exclusive with using the `domain` module",
        )?;
        super::node_errors_shared_vm::decorate_error(c, error, "ERR_DOMAIN_CALLBACK_NOT_AVAILABLE")?;
        return Err(c.throw(error));
    }
    let state = c.null_object_rooted()?;
    let state = c.retain(state)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(STATE.into(), state);
    let exports = c.object_rooted()?;
    put(c, state, STATE_EXPORTS, exports)?;
    let stack = c.array_rooted(&[])?;
    put(c, state, STATE_STACK, stack)?;
    put(c, exports, "_stack", stack)?;

    let emitter = crate::host::shared_vm::commonjs::events_module(c)?;
    let emitter_prototype = get(c, emitter, "prototype")?;
    let class = c.host_constructor_named(crate::host::shared_vm::operation!("domainConstruct"), "Domain")?;
    c.set_prototype_rooted(class, emitter)?;
    let prototype = get(c, class, "prototype")?;
    c.set_prototype_rooted(prototype, emitter_prototype)?;
    let undefined = c.undefined();
    put(c, prototype, "members", undefined)?;
    for (name, operation_name) in PROTOTYPE_METHODS {
        let function = c.host_function_named(operation(operation_name), "")?;
        put(c, prototype, name, function)?;
    }
    put(c, state, STATE_CLASS, class)?;
    put(c, exports, "Domain", class)?;
    let create = c.host_function_named(crate::host::shared_vm::operation!("domainCreate"), "createDomain")?;
    put(c, exports, "createDomain", create)?;
    put(c, exports, "create", create)?;
    let null = c.null();
    put(c, exports, ACTIVE, null)?;

    for (name, operation_name, function_name) in [
        (STATE_CAPTURE, "domainCapture", ""),
        (STATE_CLEAR, "domainClear", "domainUncaughtExceptionClear"),
        (STATE_UPDATE, "domainUpdateCapture", "updateExceptionCapture"),
    ] {
        let function = c.host_function_named(operation(operation_name), function_name)?;
        put(c, state, name, function)?;
    }
    let map = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        get(c, global, "Map")
    })?;
    let pairing = c.construct_rooted(map, map, &[])?;
    put(c, state, STATE_PAIRING, pairing)?;
    let hooks = c.object_rooted()?;
    for (name, operation_name) in HOOK_CALLBACKS {
        let function = c.host_function_named(operation(operation_name), name)?;
        put(c, hooks, name, function)?;
    }
    let async_hooks = super::async_hooks_shared_vm::module(c)?;
    let hook = call_method(c, async_hooks, "createHook", &[hooks])?;
    put(c, state, STATE_HOOK, hook)?;

    for (event, operation_name) in [("newListener", "domainProcessNewListener"), ("removeListener", "domainProcessRemoveListener")] {
        let listener = c.host_function_named(operation(operation_name), "")?;
        let event = c.string_rooted(event);
        call_method(c, process, "on", &[event, listener])?;
    }
    let require_stack = c.error_rooted("require(`domain`) at this point")?;
    let require_stack = get(c, require_stack, "stack")?;
    put(c, state, STATE_REQUIRE_STACK, require_stack)?;
    let set_capture = get(c, process, "setUncaughtExceptionCaptureCallback")?;
    put(c, state, STATE_SET_CAPTURE, set_capture)?;
    let refuse = c.host_function_named(crate::host::shared_vm::operation!("domainRefuseCapture"), "")?;
    assign(c, process, "setUncaughtExceptionCaptureCallback", refuse)?;
    let no = c.boolean(false);
    put(c, state, STATE_DEPRECATED, no)?;

    let yes = c.boolean(true);
    assign(c, emitter, "usingDomains", yes)?;
    let init = get(c, emitter, "init")?;
    put(c, state, STATE_INIT, init)?;
    let init = c.host_function_named(crate::host::shared_vm::operation!("domainEmitterInit"), "")?;
    assign(c, emitter, "init", init)?;
    let emit = get(c, emitter_prototype, "emit")?;
    put(c, state, STATE_EMIT, emit)?;
    let emit = c.host_function_named(crate::host::shared_vm::operation!("domainEmitterEmit"), "emit")?;
    assign(c, emitter_prototype, "emit", emit)?;
    let trampoline = c.host_function_named(crate::host::shared_vm::operation!("domainTopLevelCallback"), "topLevelDomainCallback")?;
    super::domain_trampoline::register(c, trampoline)?;
    Ok(exports)
}

/// `new Domain()`.
pub(crate) fn construct(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        let error = c.type_error_rooted("Class constructor Domain cannot be invoked without 'new'")?;
        return Err(c.throw(error));
    }
    let state = state(c)?;
    let emitter = crate::host::shared_vm::commonjs::events_module(c)?;
    let init = get(c, emitter, "init")?;
    c.call_rooted(init, this, &[])?;
    let members = c.array_rooted(&[])?;
    assign(c, this, "members", members)?;
    let hook = get(c, state, STATE_HOOK)?;
    call_method(c, hook, "enable", &[])?;
    let update = get(c, state, STATE_UPDATE)?;
    for event in ["removeListener", "newListener"] {
        let event = c.string_rooted(event);
        call_method(c, this, "on", &[event, update])?;
    }
    Ok(c.undefined())
}

pub(crate) fn create(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let class = get(c, state, STATE_CLASS)?;
    c.construct_rooted(class, class, &[])
}

/// Marks an error as the domain's: a non-enumerable `domain` and `domainThrown`.
fn mark_error(c: &mut Context<'_>, error: RootId, domain: RootId, thrown: bool) -> Result<(), RootedError> {
    hidden(c, error, DOMAIN, domain)?;
    let thrown = c.boolean(thrown);
    assign(c, error, "domainThrown", thrown)
}

fn emit_error(c: &mut Context<'_>, domain: RootId, error: RootId) -> Result<bool, RootedError> {
    let event = c.string_rooted(ERROR_EVENT);
    let emitted = call_method(c, domain, "emit", &[event, error])?;
    c.truthy_rooted(emitted)
}

/// `domain._errorHandler(er)`, which the capture callback calls for an uncaught error:
/// whether a listener caught it.
fn error_handler(c: &mut Context<'_>, state: RootId, domain: RootId, error: RootId) -> Result<bool, RootedError> {
    if c.is_object_rooted(error)? || c.is_callable_rooted(error)? {
        mark_error(c, error, domain, true)?;
    }
    let exports = get(c, state, STATE_EXPORTS)?;
    loop {
        let active = get(c, exports, ACTIVE)?;
        if !c.strict_equal_rooted(active, domain)? {
            break;
        }
        call_method(c, domain, "exit", &[])?;
    }
    let mut caught = false;
    if stack(c, state)?.is_empty() {
        let event = c.string_rooted(ERROR_EVENT);
        let count = call_method(c, domain, "listenerCount", &[event])?;
        if c.to_number(count)? > 0.0 {
            let set_capture = get(c, state, STATE_SET_CAPTURE)?;
            let process = process(c)?;
            let null = c.null();
            c.call_rooted(set_capture, process, &[null])?;
            let emitted = emit_error(c, domain, error);
            update_capture(c, state)?;
            caught = emitted?;
        }
    } else {
        match emit_error(c, domain, error) {
            Ok(emitted) => caught = emitted,
            Err(failure) => {
                let Some(thrown) = failure.exception else {
                    return Err(failure);
                };
                update_capture(c, state)?;
                let Some(&top) = stack(c, state)?.last() else {
                    return Err(c.throw(thrown));
                };
                set_active(c, state, top)?;
                let handler = get(c, top, "_errorHandler")?;
                let result = c.call_rooted(handler, top, &[thrown])?;
                caught = c.truthy_rooted(result)?;
            }
        }
    }
    clear(c, state)?;
    Ok(caught)
}

pub(crate) fn error_handler_method(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let error = argument(c, args, 0);
    let caught = error_handler(c, state, this, error)?;
    Ok(c.boolean(caught))
}

pub(crate) fn enter_method(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    enter(c, state, this)?;
    Ok(c.undefined())
}

pub(crate) fn exit_method(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    exit(c, state, this)?;
    Ok(c.undefined())
}

/// `domain.add(emitter)`: the emitter (or timer) joins this domain, leaving its previous
/// one; a domain does not join a domain it is (transitively) a member of.
pub(crate) fn add(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let undefined = c.undefined();
    let member = argument(c, args, 0);
    let current = get(c, member, DOMAIN)?;
    if c.strict_equal_rooted(current, this)? {
        return Ok(undefined);
    }
    if c.truthy_rooted(current)? {
        call_method(c, current, "remove", &[member])?;
    }
    let mut parent = get(c, this, DOMAIN)?;
    if c.truthy_rooted(parent)? && is_domain(c, state, member)? {
        while c.truthy_rooted(parent)? {
            if c.strict_equal_rooted(parent, member)? {
                return Ok(undefined);
            }
            parent = get(c, parent, DOMAIN)?;
        }
    }
    hidden(c, member, DOMAIN, this)?;
    let members = get(c, this, "members")?;
    call_method(c, members, "push", &[member])?;
    Ok(undefined)
}

pub(crate) fn remove(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let member = argument(c, args, 0);
    let null = c.null();
    assign(c, member, DOMAIN, null)?;
    let members = get(c, this, "members")?;
    let index = call_method(c, members, "indexOf", &[member])?;
    if c.to_number(index)? >= 0.0 {
        let one = c.number(1.0);
        call_method(c, members, "splice", &[index, one])?;
    }
    Ok(c.undefined())
}

/// `domain.run(fn, ...args)`: `fn.apply(domain, args)` inside the domain.
pub(crate) fn run(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let callback = argument(c, args, 0);
    call_method(c, this, "enter", &[])?;
    let result = c.call_rooted(callback, this, args.get(1..).unwrap_or(&[]))?;
    call_method(c, this, "exit", &[])?;
    Ok(result)
}

/// The wrapper `bind` and `intercept` return, with its domain and callback.
fn wrapper(c: &mut Context<'_>, this: RootId, args: &[RootId], operation_name: &str, name: &str) -> Result<RootId, RootedError> {
    let callback = argument(c, args, 0);
    let data = c.array_rooted(&[this, callback])?;
    c.host_function_named_with_data(operation(operation_name), name, data)
}

fn wrapped(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let data = c.host_function_data()?;
    Ok((get(c, data, "0")?, get(c, data, "1")?))
}

/// `callback.apply(receiver, args)` inside `domain`.
fn run_in(c: &mut Context<'_>, domain: RootId, callback: RootId, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    call_method(c, domain, "enter", &[])?;
    let result = c.call_rooted(callback, receiver, args)?;
    call_method(c, domain, "exit", &[])?;
    Ok(result)
}

pub(crate) fn bind(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let bound = wrapper(c, this, args, "domainRunBound", "runBound")?;
    hidden(c, bound, DOMAIN, this)?;
    Ok(bound)
}

pub(crate) fn run_bound(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (domain, callback) = wrapped(c)?;
    run_in(c, domain, callback, this, args)
}

pub(crate) fn intercept(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    wrapper(c, this, args, "domainRunIntercepted", "runIntercepted")
}

/// An intercepted callback: an `Error` first argument goes to the domain's `'error'`
/// listeners; otherwise the callback runs inside the domain without it.
pub(crate) fn run_intercepted(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (domain, callback) = wrapped(c)?;
    let first = argument(c, args, 0);
    let error_class = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        get(c, global, "Error")
    })?;
    if c.truthy_rooted(first)? && c.is_object_rooted(first)? && c.instance_of_rooted(first, error_class)? {
        assign(c, first, "domainBound", callback)?;
        let no = c.boolean(false);
        assign(c, first, "domainThrown", no)?;
        hidden(c, first, DOMAIN, domain)?;
        let event = c.string_rooted(ERROR_EVENT);
        call_method(c, domain, "emit", &[event, first])?;
        return Ok(c.undefined());
    }
    run_in(c, domain, callback, this, args.get(1..).unwrap_or(&[]))
}

/// The domain-aware `EventEmitter.init`: an emitter made in a domain (other than a domain)
/// belongs to it.
pub(crate) fn emitter_init(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let null = c.null();
    hidden(c, this, DOMAIN, null)?;
    let exports = get(c, state, STATE_EXPORTS)?;
    let active = get(c, exports, ACTIVE)?;
    if c.truthy_rooted(active)? && !is_domain(c, state, this)? {
        assign(c, this, DOMAIN, active)?;
    }
    let init = get(c, state, STATE_INIT)?;
    c.call_rooted(init, this, args)
}

/// The domain-aware `EventEmitter.prototype.emit`: an emitter's events run inside its
/// domain, and an `'error'` without listeners goes to the domain's, run outside it.
pub(crate) fn emitter_emit(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let emit = get(c, state, STATE_EMIT)?;
    let domain = get(c, this, DOMAIN)?;
    let kind = argument(c, args, 0);
    let is_error = c.string_text(kind)?.as_deref() == Some(ERROR_EVENT);
    let listened = is_error && {
        let count = call_method(c, this, "listenerCount", &[kind])?;
        c.to_number(count)? > 0.0
    };
    let process = process(c)?;
    if listened || nullish(c, domain) || c.strict_equal_rooted(this, process)? {
        return c.call_rooted(emit, this, args);
    }
    if is_error {
        let provided = argument(c, args, 1);
        let error = if args.len() > 1 && c.truthy_rooted(provided)? {
            provided
        } else {
            let error = c.error_rooted("Unhandled error.")?;
            super::node_errors_shared_vm::decorate_error(c, error, "ERR_UNHANDLED_ERROR")?;
            error
        };
        if c.is_object_rooted(error)? {
            assign(c, error, "domainEmitter", this)?;
            mark_error(c, error, domain, false)?;
        }
        let original = stack(c, state)?;
        let active = get(c, process, DOMAIN)?;
        let mut kept = original.len();
        while kept > 0 && c.strict_equal_rooted(active, original[kept - 1])? {
            kept -= 1;
        }
        truncate_stack(c, state, kept)?;
        let parent = match kept.checked_sub(1) {
            Some(top) => original[top],
            None => c.null(),
        };
        set_active(c, state, parent)?;
        update_capture(c, state)?;
        let event = c.string_rooted(ERROR_EVENT);
        call_method(c, domain, "emit", &[event, error])?;
        let restored = c.array_rooted(&original)?;
        put(c, state, STATE_STACK, restored)?;
        let exports = get(c, state, STATE_EXPORTS)?;
        put(c, exports, "_stack", restored)?;
        set_active(c, state, active)?;
        update_capture(c, state)?;
        return Ok(c.boolean(false));
    }
    run_in(c, domain, emit, this, args)
}

/// The uncaught-exception capture callback: `process.domain._errorHandler(er)`.
pub(crate) fn capture(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let process = process(c)?;
    let domain = get(c, process, DOMAIN)?;
    let error = argument(c, args, 0);
    call_method(c, domain, "_errorHandler", &[error])
}

pub(crate) fn clear_operation(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    clear(c, state)?;
    Ok(c.undefined())
}

pub(crate) fn update_capture_operation(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    update_capture(c, state)?;
    Ok(c.undefined())
}

/// `process`'s `'newListener'` listener: the domain stack's clearer stays the first
/// `'uncaughtException'` listener.
pub(crate) fn process_new_listener(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let clearer = get(c, state, STATE_CLEAR)?;
    let (name, listener) = (argument(c, args, 0), argument(c, args, 1));
    if c.string_text(name)?.as_deref() == Some(UNCAUGHT_EXCEPTION) && !c.strict_equal_rooted(listener, clearer)? {
        let process = process(c)?;
        call_method(c, process, "removeListener", &[name, clearer])?;
        call_method(c, process, "prependListener", &[name, clearer])?;
    }
    Ok(c.undefined())
}

/// `process`'s `'removeListener'` listener: the clearer goes with the last other
/// `'uncaughtException'` listener.
pub(crate) fn process_remove_listener(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let clearer = get(c, state, STATE_CLEAR)?;
    let (name, listener) = (argument(c, args, 0), argument(c, args, 1));
    if c.string_text(name)?.as_deref() == Some(UNCAUGHT_EXCEPTION) && !c.strict_equal_rooted(listener, clearer)? {
        let process = process(c)?;
        let listeners = call_method(c, process, "listeners", &[name])?;
        let listeners = c.array_like_values_rooted(listeners)?;
        if let [only] = listeners[..] {
            if c.strict_equal_rooted(only, clearer)? {
                call_method(c, process, "removeListener", &[name, clearer])?;
            }
        }
    }
    Ok(c.undefined())
}

/// `process.setUncaughtExceptionCaptureCallback` while domains own the capture callback.
pub(crate) fn refuse_capture(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let error = c.error_rooted(
        "The `domain` module is in use, which is mutually exclusive with calling process.setUncaughtExceptionCaptureCallback()",
    )?;
    super::node_errors_shared_vm::decorate_error(c, error, "ERR_DOMAIN_CANNOT_SET_UNCAUGHT_EXCEPTION_CAPTURE")?;
    let stack = get(c, error, "stack")?;
    let stack = c.to_string(stack)?;
    let require_stack = get(c, state, STATE_REQUIRE_STACK)?;
    let require_stack = c.to_string(require_stack)?;
    let stack = c.string_rooted(&format!("{stack}\n{}\n{require_stack}", "-".repeat(STACK_RULE_WIDTH)));
    assign(c, error, "stack", stack)?;
    Err(c.throw(error))
}

/// The pairing of async resources with the domain they were made in.
fn pairing(c: &mut Context<'_>, member: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let pairing = get(c, state, STATE_PAIRING)?;
    let method = intrinsic(c, "Map", member)?;
    c.call_rooted(method, pairing, args)
}

pub(crate) fn hook_init(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let process = process(c)?;
    let domain = get(c, process, DOMAIN)?;
    if nullish(c, domain) {
        return Ok(undefined);
    }
    let (id, kind, resource) = (argument(c, args, 0), argument(c, args, 1), argument(c, args, 3));
    pairing(c, "set", &[id, domain])?;
    // A promise of another realm (a vm context) gets no `domain` to escape through.
    let promise = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        get(c, global, "Promise")
    })?;
    let foreign_promise = c.string_text(kind)?.as_deref() == Some(PROMISE_RESOURCE)
        && !(c.is_object_rooted(resource)? && c.instance_of_rooted(resource, promise)?);
    if !foreign_promise && c.is_object_rooted(resource)? {
        hidden(c, resource, DOMAIN, domain)?;
    }
    Ok(undefined)
}

fn paired_domain(c: &mut Context<'_>, args: &[RootId]) -> Result<Option<RootId>, RootedError> {
    let id = argument(c, args, 0);
    let domain = pairing(c, "get", &[id])?;
    Ok((!c.rooted_value(domain).is_some_and(|value| value.is_undefined())).then_some(domain))
}

pub(crate) fn hook_before(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if let Some(domain) = paired_domain(c, args)? {
        call_method(c, domain, "enter", &[])?;
    }
    Ok(c.undefined())
}

pub(crate) fn hook_after(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if let Some(domain) = paired_domain(c, args)? {
        call_method(c, domain, "exit", &[])?;
    }
    Ok(c.undefined())
}

pub(crate) fn hook_destroy(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = argument(c, args, 0);
    pairing(c, "delete", &[id])?;
    Ok(c.undefined())
}

/// Node's `topLevelDomainCallback(cb, ...args)`, which native callbacks without an async
/// context run through: inside the receiver's domain, if it has one.
pub(crate) fn top_level_callback(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = state(c)?;
    let callback = argument(c, args, 0);
    let rest = args.get(1..).unwrap_or(&[]);
    let domain = if c.is_object_rooted(this)? { get(c, this, DOMAIN)? } else { c.undefined() };
    if !c.truthy_rooted(domain)? {
        return c.call_rooted(callback, this, rest);
    }
    let exports = get(c, state, STATE_EXPORTS)?;
    let active = get(c, exports, ACTIVE)?;
    let warned = get(c, state, STATE_DEPRECATED)?;
    if c.truthy_rooted(active)? && !c.truthy_rooted(warned)? {
        let yes = c.boolean(true);
        put(c, state, STATE_DEPRECATED, yes)?;
        let method = get(c, callback, "name")?;
        let method = c.to_string(method)?;
        let method = if method.is_empty() { "anonymous".to_owned() } else { method };
        let constructor = get(c, this, "constructor")?;
        let target = get(c, constructor, "name")?;
        let target = c.to_string(target)?;
        let message = c.string_rooted(&format!("{} (Triggered by calling {method} on {target}.)", MAKE_CALLBACK_DEPRECATION.0));
        let kind = c.string_rooted("DeprecationWarning");
        let code = c.string_rooted(MAKE_CALLBACK_DEPRECATION.1);
        let process = process(c)?;
        call_method(c, process, "emitWarning", &[message, kind, code])?;
    }
    call_method(c, domain, "enter", &[])?;
    let result = c.call_rooted(callback, this, rest)?;
    call_method(c, domain, "exit", &[])?;
    Ok(result)
}
