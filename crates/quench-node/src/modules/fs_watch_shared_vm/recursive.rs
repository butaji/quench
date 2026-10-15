//! Recursive watch policy composes the canonical public per-path watcher.
//! Private class data owns the tree in the VM heap, so callback cycles are GC-visible.
use super::{get, nullish, set};
use crate::host::NodeHost;
use quench_runtime::{BuiltinData, NativeContext, RootId, RootedError};
pub(crate) mod iterator;

#[cfg(target_os = "linux")]
const FACTORY: &str = quench_js_check::checked_js!(
    r#"(Emitter, initialize, start, close, reference, notify, iterate, next, finish, startSymbol, MapConstructor, mapGet, mapSet, mapDelete, fs, resolve, join, relative, basename) => {
 'use strict';
 class FSWatcher extends Emitter {
   #state;
   constructor(options = {}) { super(); this.#state = initialize(options); }
   [startSymbol](filename) { return start(this, this.#state, filename); }
   close() { return close(this, this.#state); }
   ref() { return reference(this.#state, true); }
   unref() { return reference(this.#state, false); }
   [Symbol.asyncIterator]() { return iterate(this, this.#state); }
 }
 return { FSWatcher, startSymbol, MapConstructor, mapGet, mapSet, mapDelete, fs, resolve, join, relative, basename,
   makeCallback: (owner, state, path, record) => (kind, filename) => notify(owner, state, path, record, kind, filename),
   makeIterator: (owner, state) => ({ next: () => next(owner, state), return: () => finish(owner, state), [Symbol.asyncIterator]() { return this; } })
 };
}"#
);

#[cfg(target_os = "linux")]
pub(super) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    parent: RootId,
    fs: RootId,
    emitter: RootId,
) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let map = get(context, global, "Map")?;
    let prototype = get(context, map, "prototype")?;
    let mut arguments = vec![emitter];
    for name in [
        "fsRecursiveInitialize",
        "fsRecursiveStart",
        "fsRecursiveClose",
        "fsRecursiveRef",
        "fsRecursiveNotify",
        "fsRecursiveIterate",
        "fsRecursiveNext",
        "fsRecursiveReturn",
    ] {
        arguments.push(context.host_function(crate::host::shared_vm::operation(name))?);
    }
    arguments.push(context.symbol_rooted(Some("kFSWatchStart")));
    arguments.push(map);
    for name in ["get", "set", "delete"] {
        arguments.push(get(context, prototype, name)?);
    }
    arguments.push(fs);
    let path = crate::modules::path_shared_vm::module(context)?;
    for name in ["resolve", "join", "relative", "basename"] {
        arguments.push(get(context, path, name)?);
    }
    let factory = context.evaluate_script_rooted(FACTORY, "node:fs/recursive-watch-facade.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(factory, undefined, &arguments)?;
    set(context, parent, "recursive", api)
}
fn api(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let parent = shared
        .borrow()
        .fs_watch
        .api
        .ok_or_else(|| RootedError::host("watch API missing"))?;
    get(context, parent, "recursive")
}
fn operation(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let api = api(context)?;
    let function = get(context, api, name)?;
    context.call_rooted(function, receiver, args)
}
fn fs_call(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let api = api(context)?;
    let fs = get(context, api, "fs")?;
    let function = get(context, fs, name)?;
    let undefined = context.undefined();
    context.call_rooted(function, undefined, args)
}
fn map_get(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    key: RootId,
) -> Result<RootId, RootedError> {
    let map = get(context, data, "files")?;
    operation(context, "mapGet", map, &[key])
}
fn map_set(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    key: RootId,
    value: RootId,
) -> Result<(), RootedError> {
    let map = get(context, data, "files")?;
    operation(context, "mapSet", map, &[key, value])?;
    Ok(())
}
fn map_delete(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    key: RootId,
) -> Result<(), RootedError> {
    let map = get(context, data, "files")?;
    operation(context, "mapDelete", map, &[key])?;
    Ok(())
}
fn files(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
) -> Result<Vec<(RootId, RootId)>, RootedError> {
    let map = get(context, data, "files")?;
    match context.builtin_data_rooted(map)? {
        Some(BuiltinData::Map(entries)) => Ok(entries),
        _ => Err(RootedError::host("recursive watch tree is not a Map")),
    }
}
fn closed(context: &mut NativeContext<'_, NodeHost>, data: RootId) -> Result<bool, RootedError> {
    let closed = get(context, data, "closed")?;
    context.truthy_rooted(closed)
}
fn method(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let method = get(context, object, name)?;
    context.call_rooted(method, object, &[])
}
fn is_enoent(
    context: &mut NativeContext<'_, NodeHost>,
    error: &RootedError,
) -> Result<bool, RootedError> {
    let Some(exception) = error.exception else {
        return Ok(false);
    };
    let code = get(context, exception, "code")?;
    Ok(context.string_text(code)?.as_deref() == Some("ENOENT"))
}
fn boolean_option(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<(), RootedError> {
    if !nullish(context, value)
        && context
            .rooted_value(value)
            .and_then(|value| value.as_bool())
            .is_none()
    {
        let error =
            crate::modules::util_shared_vm::argument_type_error(context, value, name, "boolean")?;
        return Err(crate::modules::net_options_shared_vm::validator_error(
            context,
            error,
            "ERR_INVALID_ARG_TYPE",
        )?);
    }
    Ok(())
}
pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = args.first().copied().unwrap_or_else(|| context.undefined());
    let saved = context.object_rooted()?;
    for name in [
        "persistent",
        "recursive",
        "signal",
        "encoding",
        "ignore",
        "throwIfNoEntry",
    ] {
        let value = get(context, options, name)?;
        set(context, saved, name, value)?;
    }
    for name in ["recursive", "persistent"] {
        let value = get(context, saved, name)?;
        boolean_option(context, value, &format!("options.{name}"))?;
    }
    let signal = get(context, saved, "signal")?;
    crate::modules::util_shared_vm::validate_abort_signal(context, signal, "options.signal")?;
    let throw = get(context, saved, "throwIfNoEntry")?;
    boolean_option(context, throw, "options.throwIfNoEntry")?;
    if nullish(context, throw) {
        let yes = context.boolean(true);
        set(context, saved, "throwIfNoEntry", yes)?;
    }
    let encoding = get(context, saved, "encoding")?;
    if !nullish(context, encoding) && context.string_text(encoding)?.is_none() {
        let inspect = crate::modules::util_shared_vm::internal_inspect(context)?;
        let undefined = context.undefined();
        let detail = context.call_rooted(inspect, undefined, &[encoding])?;
        let detail = context.to_string(detail)?;
        let error = context.type_error_rooted(&format!(
            "The argument 'options.encoding' is invalid. Received {detail}"
        ))?;
        crate::modules::node_errors_shared_vm::decorate_type_error(
            context,
            error,
            "ERR_INVALID_ARG_VALUE",
        )?;
        return Err(context.throw(error));
    }
    let ignore = get(context, saved, "ignore")?;
    let matcher = super::ignore_matcher(context, ignore)?;
    let data = context.object_rooted()?;
    set(context, data, "options", saved)?;
    set(context, data, "matcher", matcher)?;
    let api = api(context)?;
    let constructor = get(context, api, "MapConstructor")?;
    let map = context.construct_rooted(constructor, constructor, &[])?;
    set(context, data, "files", map)?;
    let no = context.boolean(false);
    set(context, data, "closed", no)?;
    set(context, data, "watchingFile", no)?;
    let undefined = context.undefined();
    let root = operation(context, "resolve", undefined, &[])?;
    set(context, data, "rootPath", root)?;
    Ok(data)
}
pub(super) fn watch(
    context: &mut NativeContext<'_, NodeHost>,
    filename: RootId,
    options: RootId,
    listener: RootId,
) -> Result<RootId, RootedError> {
    let api = api(context)?;
    let constructor = get(context, api, "FSWatcher")?;
    let watcher = context.construct_rooted(constructor, constructor, &[options])?;
    let symbol = get(context, api, "startSymbol")?;
    let start = context.get_property_rooted(watcher, symbol)?;
    context.call_rooted(start, watcher, &[filename])?;
    if context.truthy_rooted(listener)? {
        let add = get(context, watcher, "addListener")?;
        let change = context.string_rooted("change");
        context.call_rooted(add, watcher, &[change, listener])?;
    }
    let signal = get(context, options, "signal")?;
    super::attach_signal(context, watcher, signal)?;
    Ok(watcher)
}

pub(crate) fn promise_watch(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let filename = args.first().copied().unwrap_or_else(|| context.undefined());
    let mut options = args.get(1).copied().unwrap_or_else(|| context.undefined());
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_undefined())
    {
        options = context.object_rooted()?;
    }
    if !context.is_object_rooted(options)?
        || context.is_callable_rooted(options)?
        || context.is_array_rooted(options)?
    {
        let error = crate::modules::util_shared_vm::argument_type_error(
            context, options, "options", "object",
        )?;
        return Err(crate::modules::net_options_shared_vm::validator_error(
            context,
            error,
            "ERR_INVALID_ARG_TYPE",
        )?);
    }
    let recursive = get(context, options, "recursive")?;
    if !nullish(context, recursive) {
        let recursive = get(context, options, "recursive")?;
        boolean_option(context, recursive, "options.recursive")?;
        let recursive = get(context, options, "recursive")?;
        if context.truthy_rooted(recursive)? {
            let api = api(context)?;
            let constructor = get(context, api, "FSWatcher")?;
            let watcher = context.construct_rooted(constructor, constructor, &[options])?;
            let symbol = get(context, api, "startSymbol")?;
            let start = context.get_property_rooted(watcher, symbol)?;
            context.call_rooted(start, watcher, &[filename])?;
            let global = context.global_root()?;
            let symbol = get(context, global, "Symbol")?;
            let symbol = get(context, symbol, "asyncIterator")?;
            let iterate = context.get_property_rooted(watcher, symbol)?;
            return context.call_rooted(iterate, watcher, &[]);
        }
    }
    super::promise::create(context, filename, options)
}
pub(crate) fn start(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let watcher = args.first().copied().unwrap_or_else(|| context.undefined());
    let data = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let filename = args.get(2).copied().unwrap_or_else(|| context.undefined());
    let (_, filename) = super::path(context, filename)?;
    let undefined = context.undefined();
    let filename = operation(context, "resolve", undefined, &[filename])?;
    let result = (|| {
        let stat = fs_call(context, "statSync", &[filename])?;
        set(context, data, "rootPath", filename)?;
        let no = context.boolean(false);
        set(context, data, "closed", no)?;
        let file = method(context, stat, "isFile")?;
        set(context, data, "watchingFile", file)?;
        watch_file(context, watcher, data, filename, false)?;
        let directory = method(context, stat, "isDirectory")?;
        if context.truthy_rooted(directory)? {
            watch_folder(context, watcher, data, filename, true)?;
        }
        Ok(context.undefined())
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let options = get(context, data, "options")?;
            let throw = get(context, options, "throwIfNoEntry")?;
            if context.truthy_rooted(throw)? || !is_enoent(context, &error)? {
                if let Some(exception) = error.exception {
                    set(context, exception, "filename", filename)?;
                }
                let undefined = context.undefined();
                close(context, undefined, &[watcher, data])?;
                Err(error)
            } else {
                Ok(context.undefined())
            }
        }
    }
}
fn watch_file(
    context: &mut NativeContext<'_, NodeHost>,
    watcher: RootId,
    data: RootId,
    file: RootId,
    symbolic: bool,
) -> Result<(), RootedError> {
    if closed(context, data)? {
        return Ok(());
    }
    let existing = map_get(context, data, file)?;
    if !nullish(context, existing) {
        return Ok(());
    }
    let stat = fs_call(context, "statSync", &[file])?;
    let record = context.object_rooted()?;
    set(context, record, "stats", stat)?;
    let symbolic = context.boolean(symbolic);
    set(context, record, "symbolic", symbolic)?;
    map_set(context, data, file, record)?;
    // Linux directory watches report events for their entries. Keep regular
    // files in the same tree, but only arm directories, links and a file root.
    let directory = method(context, stat, "isDirectory")?;
    let root = get(context, data, "rootPath")?;
    if !context.truthy_rooted(directory)?
        && !context.truthy_rooted(symbolic)?
        && !context.strict_equal_rooted(file, root)?
    {
        return Ok(());
    }
    let undefined = context.undefined();
    let callback = operation(
        context,
        "makeCallback",
        undefined,
        &[watcher, data, file, record],
    )?;
    let options = get(context, data, "options")?;
    let persistent = get(context, options, "persistent")?;
    let child_options = context.object_rooted()?;
    set(context, child_options, "persistent", persistent)?;
    let child = fs_call(context, "watch", &[file, child_options, callback])?;
    set(context, record, "watcher", child)?;
    Ok(())
}
fn relative(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    file: RootId,
) -> Result<RootId, RootedError> {
    let root = get(context, data, "rootPath")?;
    let undefined = context.undefined();
    operation(context, "relative", undefined, &[root, file])
}
fn emit(
    context: &mut NativeContext<'_, NodeHost>,
    watcher: RootId,
    kind: &str,
    filename: RootId,
) -> Result<(), RootedError> {
    let emit = get(context, watcher, "emit")?;
    let change = context.string_rooted("change");
    let kind = context.string_rooted(kind);
    context.call_rooted(emit, watcher, &[change, kind, filename])?;
    Ok(())
}
fn watch_folder(
    context: &mut NativeContext<'_, NodeHost>,
    watcher: RootId,
    data: RootId,
    folder: RootId,
    initial: bool,
) -> Result<(), RootedError> {
    let result = (|| {
        let options = context.object_rooted()?;
        let yes = context.boolean(true);
        set(context, options, "withFileTypes", yes)?;
        let entries = fs_call(context, "readdirSync", &[folder, options])?;
        let iterator = context.iterator_rooted(entries)?;
        while let Some(entry) = context.iterator_step_rooted(&iterator)? {
            if closed(context, data)? {
                context.iterator_close_rooted(&iterator)?;
                break;
            }
            let name = get(context, entry, "name")?;
            let undefined = context.undefined();
            let file = operation(context, "join", undefined, &[folder, name])?;
            let relative = relative(context, data, file)?;
            let matcher = get(context, data, "matcher")?;
            if !nullish(context, matcher) {
                let ignored = context.call_rooted(matcher, undefined, &[relative])?;
                if context.truthy_rooted(ignored)? {
                    continue;
                }
            }
            let existing = map_get(context, data, file)?;
            if !nullish(context, existing) {
                continue;
            }
            if !initial {
                emit(context, watcher, "rename", relative)?;
            }
            let symbolic = method(context, entry, "isSymbolicLink")?;
            let symbolic = context.truthy_rooted(symbolic)?;
            let result = (|| {
                watch_file(context, watcher, data, file, symbolic)?;
                let directory = method(context, entry, "isDirectory")?;
                if context.truthy_rooted(directory)? {
                    let symbolic = method(context, entry, "isSymbolicLink")?;
                    if !context.truthy_rooted(symbolic)? {
                        watch_folder(context, watcher, data, file, initial)?;
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                if !is_enoent(context, &error)? {
                    return Err(error);
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        if !is_enoent(context, &error)? {
            let emit = get(context, watcher, "emit")?;
            let event = context.string_rooted("error");
            let exception = error
                .exception
                .ok_or_else(|| RootedError::host("recursive watch error lost exception"))?;
            context.call_rooted(emit, watcher, &[event, exception])?;
        }
    }
    Ok(())
}
fn close_child(
    context: &mut NativeContext<'_, NodeHost>,
    record: RootId,
) -> Result<(), RootedError> {
    let child = get(context, record, "watcher")?;
    if !nullish(context, child) {
        method(context, child, "close")?;
    }
    Ok(())
}
fn unwatch_tree(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    file: RootId,
) -> Result<(), RootedError> {
    let path = context.to_string(file)?;
    let prefix = format!("{path}{}", std::path::MAIN_SEPARATOR);
    for (key, record) in files(context, data)? {
        let name = context.to_string(key)?;
        if name == path || name.starts_with(&prefix) {
            map_delete(context, data, key)?;
            close_child(context, record)?;
        }
    }
    Ok(())
}
pub(crate) fn notify(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let watcher = args.first().copied().unwrap_or_else(|| context.undefined());
    let data = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let mut file = args.get(2).copied().unwrap_or_else(|| context.undefined());
    let mut captured = args.get(3).copied().unwrap_or_else(|| context.undefined());
    if closed(context, data)? {
        return Ok(context.undefined());
    }
    let stats = get(context, captured, "stats")?;
    let directory = method(context, stats, "isDirectory")?;
    let symbolic = get(context, captured, "symbolic")?;
    let filename = args.get(5).copied().unwrap_or_else(|| context.undefined());
    if context.truthy_rooted(directory)?
        && !context.truthy_rooted(symbolic)?
        && !nullish(context, filename)
    {
        let undefined = context.undefined();
        let child = operation(context, "join", undefined, &[file, filename])?;
        let record = map_get(context, data, child)?;
        if nullish(context, record) {
            // Rescanning registers newly discovered paths and their descendants
            // through the same entry policy used during initial discovery.
            let options = context.object_rooted()?;
            let no = context.boolean(false);
            set(context, options, "throwIfNoEntry", no)?;
            let current = fs_call(context, "statSync", &[file, options])?;
            if nullish(context, current) {
                let name = relative(context, data, file)?;
                emit(context, watcher, "rename", name)?;
                unwatch_tree(context, data, file)?;
            } else {
                watch_folder(context, watcher, data, file, false)?;
            }
            return Ok(context.undefined());
        }
        file = child;
        captured = record;
    }
    let existing = map_get(context, data, file)?;
    let previous = if nullish(context, existing) {
        context.undefined()
    } else {
        get(context, existing, "stats")?
    };
    let current = fs_call(context, "statSync", &[file]).ok();
    if let Some(current) = current {
        let record = if nullish(context, existing) {
            context.object_rooted()?
        } else {
            existing
        };
        set(context, record, "stats", current)?;
        map_set(context, data, file, record)?;
    }
    let deleted = if let Some(current) = current {
        let zero = context.number(0.0);
        let birth = get(context, current, "birthtimeMs")?;
        if context.strict_equal_rooted(birth, zero)? {
            let previous = get(context, previous, "birthtimeMs")?;
            !context.strict_equal_rooted(previous, zero)?
        } else {
            false
        }
    } else {
        true
    };
    let name = relative(context, data, file)?;
    if deleted {
        map_delete(context, data, file)?;
        close_child(context, captured)?;
        emit(context, watcher, "rename", name)?;
        unwatch_tree(context, data, file)?;
    } else {
        let current = current.expect("successful current stat");
        let root = get(context, data, "rootPath")?;
        let watching_file = get(context, data, "watchingFile")?;
        if context.strict_equal_rooted(file, root)? && context.truthy_rooted(watching_file)? {
            let undefined = context.undefined();
            let name = operation(context, "basename", undefined, &[file])?;
            emit(context, watcher, "change", name)?;
        } else {
            let symbolic = if nullish(context, existing) {
                context.boolean(false)
            } else {
                get(context, existing, "symbolic")?
            };
            if context.truthy_rooted(symbolic)? {
                emit(context, watcher, "rename", name)?;
            } else {
                let directory = method(context, current, "isDirectory")?;
                if context.truthy_rooted(directory)? {
                    watch_folder(context, watcher, data, file, false)?;
                } else {
                    emit(context, watcher, "change", name)?;
                }
            }
        }
    }
    Ok(context.undefined())
}
pub(crate) fn close(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let watcher = args.first().copied().unwrap_or_else(|| context.undefined());
    let data = args.get(1).copied().unwrap_or_else(|| context.undefined());
    if closed(context, data)? {
        return Ok(context.undefined());
    }
    let yes = context.boolean(true);
    set(context, data, "closed", yes)?;
    for (key, record) in files(context, data)? {
        close_child(context, record)?;
        map_delete(context, data, key)?;
    }
    let emit = get(context, watcher, "emit")?;
    let close = context.string_rooted("close");
    context.call_rooted(emit, watcher, &[close])?;
    Ok(context.undefined())
}
pub(crate) fn reference(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = args.first().copied().unwrap_or_else(|| context.undefined());
    let reference = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let name = if context.truthy_rooted(reference)? {
        "ref"
    } else {
        "unref"
    };
    for (_, record) in files(context, data)? {
        let child = get(context, record, "watcher")?;
        if !nullish(context, child) {
            method(context, child, name)?;
        }
    }
    Ok(context.undefined())
}
