//! `MockTracker` and `MockFunctionContext`: mock functions record each call and run the
//! current implementation; method mocks replace a property and restore its descriptor.
use super::{accessor, function, get, put, with_state, Context, NodeId};
use crate::modules::private_slots;
use quench_runtime::{RootId, RootedError};

const MOCKS: &str = "\0quench:test:mocks";
const CALLS: &str = "\0quench:mock:calls";
const IMPLEMENTATION: &str = "\0quench:mock:implementation";
const ONCE: &str = "\0quench:mock:once";
/// `[object, key, descriptor]` a method mock restores.
const RESTORE: &str = "\0quench:mock:restore";

const TRACKER_METHODS: [(&str, &str); 4] = [
    ("fn", "nodeTestMockFn"),
    ("method", "nodeTestMockMethod"),
    ("reset", "nodeTestMockReset"),
    ("restoreAll", "nodeTestMockRestoreAll"),
];
const CONTEXT_METHODS: [(&str, &str); 5] = [
    ("callCount", "nodeTestMockCallCount"),
    ("mockImplementation", "nodeTestMockImplementation"),
    ("mockImplementationOnce", "nodeTestMockImplementationOnce"),
    ("resetCalls", "nodeTestMockResetCalls"),
    ("restore", "nodeTestMockRestore"),
];

fn slot(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    match private_slots::read(c, object, name)? {
        Some(value) => Ok(value),
        None => {
            let error = c.type_error_rooted("Illegal invocation")?;
            crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
            Err(c.throw(error))
        }
    }
}

fn prototype(c: &mut Context<'_>, methods: &[(&str, &str)], getter: Option<(&str, &str)>) -> Result<RootId, RootedError> {
    let prototype = c.object_rooted()?;
    let undefined = c.undefined();
    for (name, operation) in methods {
        let method = function(c, operation, name, undefined)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, method, true, false, true)?;
    }
    if let Some((name, operation)) = getter {
        let getter = function(c, operation, &format!("get {name}"), undefined)?;
        accessor(c, prototype, name, getter)?;
    }
    c.retain(prototype)
}

/// A new MockTracker.
pub(super) fn tracker(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let prototype = match with_state(c, |state| state.tracker_prototype) {
        Some(prototype) => prototype,
        None => {
            let prototype = prototype(c, &TRACKER_METHODS, None)?;
            with_state(c, |state| state.tracker_prototype = Some(prototype));
            prototype
        }
    };
    let tracker = c.object_rooted()?;
    c.set_prototype_rooted(tracker, prototype)?;
    let mocks = c.array_rooted(&[])?;
    private_slots::write(c, tracker, MOCKS, mocks)?;
    c.retain(tracker)
}

/// `t.mock`: the test's tracker, reset when the test finishes.
pub(super) fn test_tracker(c: &mut Context<'_>, id: NodeId) -> Result<RootId, RootedError> {
    if let Some(tracker) = with_state(c, |state| state.tree[id].mock) {
        return Ok(tracker);
    }
    let tracker = tracker(c)?;
    with_state(c, |state| state.tree[id].mock = Some(tracker));
    Ok(tracker)
}

fn context_prototype(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    const CACHE: &str = "\0quench:test:mockContextPrototype";
    if let Some(prototype) = c.host_mut().shared_state().borrow().builtin_cache.get(CACHE).copied() {
        return Ok(prototype);
    }
    let prototype = prototype(c, &CONTEXT_METHODS, Some(("calls", "nodeTestMockCalls")))?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(CACHE.to_owned(), prototype);
    Ok(prototype)
}

/// A mock function over `original` running `implementation`, tracked by `tracker`.
fn create(c: &mut Context<'_>, tracker: RootId, original: RootId, implementation: RootId) -> Result<RootId, RootedError> {
    let mocks = slot(c, tracker, MOCKS)?;
    let prototype = context_prototype(c)?;
    let context = c.object_rooted()?;
    c.set_prototype_rooted(context, prototype)?;
    let calls = c.array_rooted(&[])?;
    private_slots::write(c, context, CALLS, calls)?;
    private_slots::write(c, context, IMPLEMENTATION, implementation)?;
    let once = c.null_object_rooted()?;
    private_slots::write(c, context, ONCE, once)?;
    let undefined = c.undefined();
    private_slots::write(c, context, RESTORE, undefined)?;
    let name = if c.is_callable_rooted(original)? {
        let name = get(c, original, "name")?;
        c.string_text(name)?.unwrap_or_default()
    } else {
        String::new()
    };
    let mock = function(c, "nodeTestMockCall", &name, context)?;
    let key = c.string_rooted("mock");
    c.define_data_property_rooted(mock, key, context, false, false, true)?;
    let push = get(c, mocks, "push")?;
    c.call_rooted(push, mocks, &[context])?;
    Ok(mock)
}

fn callable_or(c: &mut Context<'_>, value: Option<RootId>, fallback: RootId) -> Result<RootId, RootedError> {
    match value {
        Some(value) if c.is_callable_rooted(value)? => Ok(value),
        _ => Ok(fallback),
    }
}

/// `mock.fn([original[, implementation]][, options])`.
pub(crate) fn fn_mock(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let noop = function(c, "nodeTestMockNoop", "", undefined)?;
    let original = callable_or(c, args.first().copied(), noop)?;
    let implementation = callable_or(c, args.get(1).copied(), original)?;
    create(c, this, original, implementation)
}

pub(crate) fn noop(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}

/// `mock.method(object, methodName[, implementation][, options])`.
pub(crate) fn method_mock(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let object = args.first().copied().unwrap_or(undefined);
    if !c.is_object_rooted(object)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, object, "object", "object")?);
    }
    let key = args.get(1).copied().unwrap_or(undefined);
    let mut descriptor = undefined;
    let mut holder = object;
    while c.is_object_rooted(holder)? {
        descriptor = c.own_property_descriptor_rooted(holder, key)?;
        if !c.rooted_value(descriptor).is_some_and(|value| value.is_undefined()) {
            break;
        }
        holder = c.prototype_rooted(holder)?;
    }
    let original = if c.is_object_rooted(descriptor)? { get(c, descriptor, "value")? } else { undefined };
    if !c.is_callable_rooted(original)? {
        return Err(crate::modules::util_shared_vm::argument_value_error(c, key, "methodName", "must be a method")?);
    }
    let implementation = callable_or(c, args.get(2).copied(), original)?;
    let mock = create(c, this, original, implementation)?;
    let replacement = c.object_rooted()?;
    put(c, replacement, "value", mock)?;
    for flag in ["writable", "enumerable", "configurable"] {
        let value = get(c, descriptor, flag)?;
        put(c, replacement, flag, value)?;
    }
    c.define_property_descriptor_rooted(object, key, replacement)?;
    let context = get(c, mock, "mock")?;
    let restore = c.array_rooted(&[object, key, descriptor])?;
    private_slots::write(c, context, RESTORE, restore)?;
    Ok(mock)
}

fn each_mock(c: &mut Context<'_>, tracker: RootId) -> Result<Vec<RootId>, RootedError> {
    let mocks = slot(c, tracker, MOCKS)?;
    c.array_like_values_rooted(mocks)
}

pub(crate) fn tracker_reset(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    for context in each_mock(c, this)? {
        restore(c, context)?;
    }
    let mocks = c.array_rooted(&[])?;
    private_slots::write(c, this, MOCKS, mocks)?;
    Ok(c.undefined())
}

pub(crate) fn tracker_restore_all(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    for context in each_mock(c, this)? {
        restore(c, context)?;
    }
    Ok(c.undefined())
}

fn restore(c: &mut Context<'_>, context: RootId) -> Result<(), RootedError> {
    let record = slot(c, context, RESTORE)?;
    if !c.is_object_rooted(record)? {
        return Ok(());
    }
    let object = get(c, record, "0")?;
    let key = get(c, record, "1")?;
    let descriptor = get(c, record, "2")?;
    c.define_property_descriptor_rooted(object, key, descriptor)?;
    let undefined = c.undefined();
    private_slots::write(c, context, RESTORE, undefined)?;
    Ok(())
}

/// A call of a mock function: the next once-implementation or the current one, recorded.
pub(crate) fn mock_call(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let context = c.host_function_data()?;
    let calls = slot(c, context, CALLS)?;
    let count = get(c, calls, "length")?;
    let once = slot(c, context, ONCE)?;
    let pending = c.get_property_rooted(once, count)?;
    let implementation = if c.is_callable_rooted(pending)? {
        c.delete_property_rooted(once, count)?;
        pending
    } else {
        slot(c, context, IMPLEMENTATION)?
    };
    let arguments = c.array_rooted(args)?;
    let stack = c.error_rooted("")?;
    let result = c.call_rooted(implementation, this, args);
    let undefined = c.undefined();
    let (error, value) = match &result {
        Ok(value) => (undefined, *value),
        Err(RootedError { exception: Some(error), .. }) => (*error, undefined),
        Err(_) => (undefined, undefined),
    };
    let call = c.object_rooted()?;
    for (name, value) in [
        ("arguments", arguments),
        ("error", error),
        ("result", value),
        ("stack", stack),
        ("target", undefined),
        ("this", this),
    ] {
        put(c, call, name, value)?;
    }
    let push = get(c, calls, "push")?;
    c.call_rooted(push, calls, &[call])?;
    result
}

pub(crate) fn mock_calls(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let calls = slot(c, this, CALLS)?;
    let values = c.array_like_values_rooted(calls)?;
    c.array_rooted(&values)
}

pub(crate) fn mock_call_count(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let calls = slot(c, this, CALLS)?;
    get(c, calls, "length")
}

pub(crate) fn mock_implementation(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    slot(c, this, CALLS)?;
    let undefined = c.undefined();
    let implementation = args.first().copied().unwrap_or(undefined);
    if !c.is_callable_rooted(implementation)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, implementation, "implementation", "function")?);
    }
    private_slots::write(c, this, IMPLEMENTATION, implementation)?;
    Ok(undefined)
}

pub(crate) fn mock_implementation_once(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let calls = slot(c, this, CALLS)?;
    let undefined = c.undefined();
    let implementation = args.first().copied().unwrap_or(undefined);
    if !c.is_callable_rooted(implementation)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, implementation, "implementation", "function")?);
    }
    let call = match args.get(1).copied().filter(|call| !c.rooted_value(*call).is_some_and(|value| value.is_undefined())) {
        Some(call) => call,
        None => get(c, calls, "length")?,
    };
    let once = slot(c, this, ONCE)?;
    c.set_property_rooted(once, call, implementation, once)?;
    Ok(undefined)
}

pub(crate) fn mock_reset_calls(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    slot(c, this, CALLS)?;
    let calls = c.array_rooted(&[])?;
    private_slots::write(c, this, CALLS, calls)?;
    Ok(c.undefined())
}

pub(crate) fn mock_restore(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    restore(c, this)?;
    Ok(c.undefined())
}
