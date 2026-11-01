//! The classes the watchers present (`FSWatcher`, `FSEvent`, `StatWatcher`, the recursive
//! `FSWatcher`) and the adapters that route their methods to the host operations, which
//! take the receiver as their first argument.

use crate::modules::stream_shared_vm::{arg, get, put, Context, Outcome};
use quench_runtime::{RootId, RootedError};

fn operation(name: &str) -> quench_runtime::HostFunctionId {
    crate::host::shared_vm::operation(name)
}

/// The values a function's data array holds.
fn data(c: &mut Context<'_>) -> Result<Vec<RootId>, RootedError> {
    let data = c.host_function_data()?;
    c.array_like_values_rooted(data)
}

/// `native(this, ...bound, ...args)`: a method over a host operation taking its receiver.
pub(crate) fn receiver_call(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let values = data(c)?;
    let mut forwarded = vec![this];
    forwarded.extend_from_slice(&values[1..]);
    forwarded.extend_from_slice(args);
    let undefined = c.undefined();
    c.call_rooted(values[0], undefined, &forwarded)
}

/// `native(...bound, ...args)`.
pub(crate) fn bound_call(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let values = data(c)?;
    let mut forwarded = values[1..].to_vec();
    forwarded.extend_from_slice(args);
    let undefined = c.undefined();
    c.call_rooted(values[0], undefined, &forwarded)
}

fn bound(c: &mut Context<'_>, native: RootId, values: &[RootId], name: &str) -> Outcome {
    let mut captured = vec![native];
    captured.extend_from_slice(values);
    let environment = c.array_rooted(&captured)?;
    c.host_function_named_with_data(operation("fsWatchBoundCall"), name, environment)
}

/// `(...values) => (...args) => native(...values, ...args)`: a callback factory.
pub(crate) fn bind(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let native = c.host_function_data()?;
    bound(c, native, args, "")
}

/// `(...values) => ({ next: () => next(...values), return: () => finish(...values),
/// [Symbol.asyncIterator]() { return this; } })`.
pub(crate) fn make_iterator(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let values = data(c)?;
    let iterator = c.object_rooted()?;
    for (name, native) in [("next", values[0]), ("return", values[1])] {
        let function = bound(c, native, args, name)?;
        put(c, iterator, name, function)?;
    }
    let key = crate::modules::fs_shared_vm::classes::well_known_symbol(c, "asyncIterator", "Symbol.asyncIterator")?;
    let identity = c.host_function_named(operation("fsDirIteratorSelf"), "[Symbol.asyncIterator]")?;
    c.set_property_rooted(iterator, key, identity, iterator)?;
    Ok(iterator)
}

/// A method `key` on `prototype` running `native(this, ...bound, ...args)`.
fn receiver_method(c: &mut Context<'_>, prototype: RootId, key: &str, native: &str, bound: &[RootId]) -> Result<(), RootedError> {
    let native = c.host_function(operation(native))?;
    let mut captured = vec![native];
    captured.extend_from_slice(bound);
    let environment = c.array_rooted(&captured)?;
    let method = c.host_function_named_with_data(operation("fsWatchReceiverCall"), key, environment)?;
    put(c, prototype, key, method)
}

/// A constructor whose instances are `EventEmitter`s, with its prototype.
fn emitter_class(c: &mut Context<'_>, construct: &str, name: &str) -> Result<(RootId, RootId), RootedError> {
    let global = c.global_root()?;
    let emitter = get(c, global, "__nodeEventEmitter")?;
    let class = c.host_constructor_named(operation(construct), name)?;
    let prototype = get(c, class, "prototype")?;
    let parent = get(c, emitter, "prototype")?;
    c.set_prototype_rooted(prototype, parent)?;
    c.set_prototype_rooted(class, emitter)?;
    Ok((class, prototype))
}

/// `EventEmitter.call(this)`, then `this[name] = null` for each of `fields`.
fn initialize_emitter(c: &mut Context<'_>, this: RootId, fields: &[&str]) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let emitter = get(c, global, "__nodeEventEmitter")?;
    c.call_rooted(emitter, this, &[])?;
    let null = c.null();
    for name in fields {
        put(c, this, name, null)?;
    }
    Ok(())
}

/// `function FSWatcher() { EventEmitter.call(this); this._handle = null; this._ignoreMatcher = null; }`
pub(crate) fn construct_watcher(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    initialize_emitter(c, this, &["_handle", "_ignoreMatcher"])?;
    Ok(c.undefined())
}

/// `function FSEvent() {}`
pub(crate) fn construct_event(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Ok(c.undefined())
}

/// `function StatWatcher() { EventEmitter.call(this); this._handle = null; }`
pub(crate) fn construct_stat_watcher(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    initialize_emitter(c, this, &["_handle"])?;
    Ok(c.undefined())
}

/// The watch API the watchers read their classes and helpers from.
pub(crate) fn watch_api(c: &mut Context<'_>) -> Outcome {
    let api = c.object_rooted()?;
    let (watcher, prototype) = emitter_class(c, "fsWatcherConstruct", "FSWatcher")?;
    let yes = c.boolean(true);
    let no = c.boolean(false);
    receiver_method(c, prototype, "close", "fsWatchClose", &[])?;
    receiver_method(c, prototype, "ref", "fsWatchRef", &[yes])?;
    receiver_method(c, prototype, "unref", "fsWatchRef", &[no])?;
    put(c, api, "FSWatcher", watcher)?;
    let event = c.host_constructor_named(operation("fsEventConstruct"), "FSEvent")?;
    let prototype = get(c, event, "prototype")?;
    receiver_method(c, prototype, "close", "fsWatchHandleClose", &[])?;
    receiver_method(c, prototype, "ref", "fsWatchHandleRef", &[yes])?;
    receiver_method(c, prototype, "unref", "fsWatchHandleRef", &[no])?;
    receiver_method(c, prototype, "hasRef", "fsWatchHandleHasRef", &[])?;
    let native = c.host_function(operation("fsWatchHandleInitialized"))?;
    let environment = c.array_rooted(&[native])?;
    let getter = c.host_function_named_with_data(operation("fsWatchReceiverCall"), "get initialized", environment)?;
    crate::modules::fs_shared_vm::classes::accessor(c, prototype, "initialized", getter, None, false)?;
    put(c, api, "FSEvent", event)?;
    let watch = c.host_function_named(operation("fsWatch"), "watch")?;
    put(c, api, "watch", watch)?;
    let recursive = recursive_api(c)?;
    put(c, api, "recursive", recursive)?;
    let promise = promise_api(c)?;
    put(c, api, "promise", promise)?;
    Ok(api)
}

/// The recursive watcher's private state.
fn recursive_state(c: &mut Context<'_>, this: RootId) -> Outcome {
    let key = state_key(c)?;
    c.get_property_rooted(this, key)
}

fn state_key(c: &mut Context<'_>) -> Outcome {
    const KEY: &str = "\0fs:recursiveWatcherState";
    let shared = c.host_mut().shared_state();
    if let Some(symbol) = shared.borrow().builtin_cache.get(KEY).copied() {
        return Ok(symbol);
    }
    let symbol = c.private_symbol_rooted(Some("#state"));
    let retained = c.retain(symbol)?;
    shared.borrow_mut().builtin_cache.insert(KEY.to_owned(), retained);
    Ok(symbol)
}

/// `class FSWatcher extends EventEmitter { constructor(options = {}) { super(); this.#state = initialize(options); } }`
pub(crate) fn construct_recursive_watcher(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    initialize_emitter(c, this, &[])?;
    let options = arg(c, args, 0);
    let options = if crate::modules::stream_shared_vm::is_undefined(c, options) { c.object_rooted()? } else { options };
    let undefined = c.undefined();
    let state = super::recursive::initialize(c, undefined, &[options])?;
    let key = state_key(c)?;
    c.set_property_rooted(this, key, state, this)?;
    Ok(c.undefined())
}

/// A recursive watcher method: `native(this, this.#state, ...args)`, or without the
/// receiver `native(this.#state, ...bound)`.
pub(crate) fn recursive_method(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let values = data(c)?;
    let with_receiver = c.truthy_rooted(values[1])?;
    let state = recursive_state(c, this)?;
    let mut forwarded = if with_receiver { vec![this, state] } else { vec![state] };
    forwarded.extend_from_slice(&values[2..]);
    forwarded.extend_from_slice(args);
    let undefined = c.undefined();
    c.call_rooted(values[0], undefined, &forwarded)
}

fn recursive_api(c: &mut Context<'_>) -> Outcome {
    let api = c.object_rooted()?;
    let (watcher, prototype) = emitter_class(c, "fsRecursiveWatcherConstruct", "FSWatcher")?;
    let start = c.symbol_rooted(Some("kFSWatchStart"));
    let iterator = crate::modules::fs_shared_vm::classes::well_known_symbol(c, "asyncIterator", "Symbol.asyncIterator")?;
    let yes = c.boolean(true);
    let no = c.boolean(false);
    for (key, native, with_receiver, bound) in [
        (start, "fsRecursiveStart", yes, None),
        (c.string_rooted("close"), "fsRecursiveClose", yes, None),
        (c.string_rooted("ref"), "fsRecursiveRef", no, Some(yes)),
        (c.string_rooted("unref"), "fsRecursiveRef", no, Some(no)),
        (iterator, "fsRecursiveIterate", yes, None),
    ] {
        let native = c.host_function(operation(native))?;
        let mut captured = vec![native, with_receiver];
        captured.extend(bound);
        let environment = c.array_rooted(&captured)?;
        let method = c.host_function_with_data(operation("fsRecursiveMethod"), environment)?;
        c.set_property_rooted(prototype, key, method, prototype)?;
    }
    put(c, api, "FSWatcher", watcher)?;
    put(c, api, "startSymbol", start)?;
    let notify = c.host_function(operation("fsRecursiveNotify"))?;
    let make_callback = c.host_function_with_data(operation("fsWatchBind"), notify)?;
    put(c, api, "makeCallback", make_callback)?;
    let next = c.host_function(operation("fsRecursiveNext"))?;
    let finish = c.host_function(operation("fsRecursiveReturn"))?;
    let natives = c.array_rooted(&[next, finish])?;
    let make_iterator = c.host_function_with_data(operation("fsWatchMakeIterator"), natives)?;
    put(c, api, "makeIterator", make_iterator)?;
    let global = c.global_root()?;
    let map = get(c, global, "Map")?;
    put(c, api, "MapConstructor", map)?;
    let map_prototype = get(c, map, "prototype")?;
    for (name, method) in [("mapGet", "get"), ("mapSet", "set"), ("mapDelete", "delete")] {
        let function = get(c, map_prototype, method)?;
        put(c, api, name, function)?;
    }
    let path = crate::modules::path_shared_vm::module(c)?;
    for name in ["resolve", "join", "relative", "basename"] {
        let function = get(c, path, name)?;
        put(c, api, name, function)?;
    }
    Ok(api)
}

fn promise_api(c: &mut Context<'_>) -> Outcome {
    let api = c.object_rooted()?;
    let global = c.global_root()?;
    let promise = get(c, global, "Promise")?;
    put(c, api, "PromiseConstructor", promise)?;
    let array = get(c, global, "Array")?;
    let array_prototype = get(c, array, "prototype")?;
    for name in ["push", "shift"] {
        let function = get(c, array_prototype, name)?;
        put(c, api, name, function)?;
    }
    let error = get(c, global, "Error")?;
    let abort = c.host_constructor_named(operation("fsEventConstruct"), "AbortError")?;
    let prototype = get(c, abort, "prototype")?;
    let parent = get(c, error, "prototype")?;
    c.set_prototype_rooted(prototype, parent)?;
    c.set_prototype_rooted(abort, error)?;
    put(c, api, "AbortError", abort)?;
    let next = c.host_function(operation("fsPromiseWatchNext"))?;
    let finish = c.host_function(operation("fsPromiseWatchReturn"))?;
    let natives = c.array_rooted(&[next, finish])?;
    let make_iterator = c.host_function_with_data(operation("fsWatchMakeIterator"), natives)?;
    put(c, api, "makeIterator", make_iterator)?;
    Ok(api)
}

/// The stat-watcher API: `StatWatcher`, `watchFile` and `unwatchFile`.
pub(crate) fn stat_watch_api(c: &mut Context<'_>) -> Outcome {
    let api = c.object_rooted()?;
    let (watcher, prototype) = emitter_class(c, "fsStatWatcherConstruct", "StatWatcher")?;
    let start = c.host_function_named(operation("fsWatchNoop"), "start")?;
    put(c, prototype, "start", start)?;
    let yes = c.boolean(true);
    let no = c.boolean(false);
    receiver_method(c, prototype, "stop", "fsStatWatchStop", &[])?;
    receiver_method(c, prototype, "ref", "fsStatWatchRef", &[yes])?;
    receiver_method(c, prototype, "unref", "fsStatWatchRef", &[no])?;
    put(c, api, "StatWatcher", watcher)?;
    let watch = c.host_function_named(operation("fsStatWatch"), "watchFile")?;
    put(c, api, "watchFile", watch)?;
    let unwatch = c.host_function_named(operation("fsStatUnwatch"), "unwatchFile")?;
    put(c, api, "unwatchFile", unwatch)?;
    Ok(api)
}

pub(crate) fn noop(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Ok(c.undefined())
}
