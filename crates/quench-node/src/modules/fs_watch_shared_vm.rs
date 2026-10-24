//! Native filesystem-event ownership. Linux uses the same masks as libuv.
use crate::host::NodeHost;
use quench_runtime::{
    BuiltinBrand, HostWeakRoot, NativeContext, RootId, RootedError, Runtime, Value,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod kernel;
pub(crate) mod promise;
pub(crate) mod recursive;
#[cfg(target_os = "linux")]
use kernel::{EventKind, WatchDescriptor};
#[cfg(not(target_os = "linux"))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct WatchDescriptor(i32);
#[cfg(not(target_os = "linux"))]
#[allow(dead_code)] // Backend is not installed on these hosts; preserve their prior fallback.
#[derive(Clone, Copy)]
enum EventKind {
    Change,
    Rename,
}

const FIRST_WATCH_ID: u64 = 1;
const MAX_WATCH_ID: u64 = (1_u64 << f64::MANTISSA_DIGITS) - 1;
#[cfg(target_os = "linux")]
const FACTORY: &str = quench_js_check::checked_js!(
    r#"(Emitter, close, reference, hClose, hReference, hHasRef, hInitialized, nativeWatch) => {
 function FSWatcher() { Emitter.call(this); this._handle = null; this._ignoreMatcher = null; }
 Object.setPrototypeOf(FSWatcher.prototype, Emitter.prototype);
 Object.setPrototypeOf(FSWatcher, Emitter);
 FSWatcher.prototype.close = function() { 'use strict'; return close(this); };
 FSWatcher.prototype.ref = function() { 'use strict'; return reference(this, true); };
 FSWatcher.prototype.unref = function() { 'use strict'; return reference(this, false); };
 function FSEvent() {}
 FSEvent.prototype.close = function() { 'use strict'; return hClose(this); };
 FSEvent.prototype.ref = function() { 'use strict'; return hReference(this, true); };
 FSEvent.prototype.unref = function() { 'use strict'; return hReference(this, false); };
 FSEvent.prototype.hasRef = function() { 'use strict'; return hHasRef(this); };
 Object.defineProperty(FSEvent.prototype, 'initialized', { get: function() { return hInitialized(this); } });
 return { FSWatcher, FSEvent, watch: function watch(filename, options, listener) { 'use strict'; return nativeWatch(filename, options, listener); } };
}"#
);

#[derive(Clone, Copy)]
enum WatchTarget {
    Watcher(RootId),
    Handle(RootId),
}
impl WatchTarget {
    fn root(self) -> RootId {
        match self {
            Self::Watcher(root) | Self::Handle(root) => root,
        }
    }
    fn with_root(self, root: RootId) -> Self {
        match self {
            Self::Watcher(_) => Self::Watcher(root),
            Self::Handle(_) => Self::Handle(root),
        }
    }
}
struct ActiveWatch {
    descriptor: WatchDescriptor,
    target: WatchTarget,
    encoding: String,
}
enum Phase {
    Unstarted,
    Watching(ActiveWatch),
    Stopped,
}
struct Handle {
    weak: HostWeakRoot,
    referenced: bool,
    phase: Phase,
}
struct Group {
    #[cfg(target_os = "linux")]
    basename: Vec<u8>,
    owners: Vec<u64>,
}
struct Delivery {
    owner: u64,
    kind: EventKind,
    name: Vec<u8>,
}
pub(crate) struct State {
    next_id: u64,
    api: Option<RootId>,
    dispatch: Option<RootId>,
    handles: HashMap<u64, Handle>,
    groups: HashMap<WatchDescriptor, Group>,
    deliveries: VecDeque<Delivery>,
    #[cfg(target_os = "linux")]
    kernel: kernel::Kernel,
}
impl Default for State {
    fn default() -> Self {
        Self {
            next_id: FIRST_WATCH_ID,
            api: None,
            dispatch: None,
            handles: HashMap::new(),
            groups: HashMap::new(),
            deliveries: VecDeque::new(),
            #[cfg(target_os = "linux")]
            kernel: kernel::Kernel::default(),
        }
    }
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn nullish(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    fs: RootId,
) -> Result<(), RootedError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (context, fs);
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let global = context.global_root()?;
        let emitter = get(context, global, "__nodeEventEmitter")?;
        let mut functions = vec![emitter];
        for name in [
            "fsWatchClose",
            "fsWatchRef",
            "fsWatchHandleClose",
            "fsWatchHandleRef",
            "fsWatchHandleHasRef",
            "fsWatchHandleInitialized",
            "fsWatch",
        ] {
            functions.push(context.host_function(crate::host::shared_vm::operation(name))?);
        }
        let factory = context.evaluate_script_rooted(FACTORY, "node:fs/watch-facade.js")?;
        let undefined = context.undefined();
        let api = context.call_rooted(factory, undefined, &functions)?;
        recursive::install(context, api, fs, emitter)?;
        promise::install(context, api)?;
        let regexp = get(context, global, "RegExp")?;
        let prototype = get(context, regexp, "prototype")?;
        let exec = get(context, prototype, "exec")?;
        set(context, api, "regexpExec", exec)?;
        set(context, api, "regexpConstructor", regexp)?;
        let watch = get(context, api, "watch")?;
        set(context, fs, "watch", watch)?;
        let api = context.retain(api)?;
        let dispatch =
            context.host_function(crate::host::shared_vm::operation("fsWatchDispatch"))?;
        let dispatch = context.retain(dispatch)?;
        let state = context.host_mut().shared_state();
        let mut state = state.borrow_mut();
        state.fs_watch.api = Some(api);
        state.fs_watch.dispatch = Some(dispatch);
        Ok(())
    }
}

fn clone_options(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootId, RootedError> {
    let options = context.object_rooted()?;
    if nullish(context, value) || context.is_callable_rooted(value)? {
        return Ok(options);
    }
    if context.string_text(value)?.is_some() {
        set(context, options, "encoding", value)?;
        return Ok(options);
    }
    if !context.is_object_rooted(value)? {
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            context,
            value,
            "options",
            "of type string or an instance of Object",
        )?);
    }
    // getOptions validates original properties before copyObject enumerates them.
    let encoding = get(context, value, "encoding")?;
    if context.string_text(encoding)?.as_deref() != Some("buffer") {
        let encoding = get(context, value, "encoding")?;
        validate_encoding(context, encoding)?;
    }
    let signal = get(context, value, "signal")?;
    if !context
        .rooted_value(signal)
        .is_some_and(|value| value.is_undefined())
    {
        let signal = get(context, value, "signal")?;
        super::util_shared_vm::validate_abort_signal(context, signal, "options.signal")?;
    }
    // copyObject uses for-in: inherited enumerable strings and nonenumerable shadows.
    let mut current = value;
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    while context.is_object_rooted(current)? && !nullish(context, current) {
        for key in context.own_keys_rooted(current)? {
            let Some(name) = context.string_text(key)? else {
                continue;
            };
            if !seen.insert(name) {
                continue;
            }
            let descriptor = context.own_property_descriptor_rooted(current, key)?;
            if context.is_object_rooted(descriptor)? && !nullish(context, descriptor) {
                let enumerable = get(context, descriptor, "enumerable")?;
                if context.truthy_rooted(enumerable)? {
                    keys.push(key);
                }
            }
        }
        current = context.prototype_rooted(current)?;
    }
    for key in keys {
        let entry = context.get_property_rooted(value, key)?;
        context.set_property_rooted(options, key, entry, options)?;
    }
    Ok(options)
}

pub(crate) fn event_binding(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let Some(api) = shared.borrow().fs_watch.api else {
        return Ok(context.undefined());
    };
    let constructor = get(context, api, "FSEvent")?;
    let binding = context.object_rooted()?;
    set(context, binding, "FSEvent", constructor)?;
    Ok(binding)
}
fn validate_encoding(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    if !context.truthy_rooted(value)? {
        return Ok("utf8".into());
    }
    if let Some(name) = context.string_text(value)? {
        if name == "buffer" {
            return Ok(name);
        }
        if let Some(name) = super::buffer_codec::canonical_encoding(&name) {
            return Ok(name.into());
        }
    }
    let inspect = super::util_shared_vm::internal_inspect(context)?;
    let undefined = context.undefined();
    let detail = context.call_rooted(inspect, undefined, &[value])?;
    let detail = context.to_string(detail)?;
    let error = context.type_error_rooted(&format!(
        "The argument 'encoding' is invalid encoding. Received {detail}"
    ))?;
    super::node_errors_shared_vm::decorate_type_error(context, error, "ERR_INVALID_ARG_VALUE")?;
    Err(context.throw(error))
}
const MATCHER_FACADE: &str = quench_js_check::checked_js!("native => filename => native(filename)");
fn wrap_matcher(
    context: &mut NativeContext<'_, NodeHost>,
    native: RootId,
) -> Result<RootId, RootedError> {
    let factory =
        context.evaluate_script_rooted(MATCHER_FACADE, "node:fs/watch-ignore-forward.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[native])
}
fn ignore_matcher(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootId, RootedError> {
    if nullish(context, value) {
        return Ok(context.null());
    }
    let is_array = context.is_array_rooted(value)?;
    let values = if is_array {
        let length = get(context, value, "length")?;
        let length = context
            .rooted_value(length)
            .and_then(Value::as_number)
            .unwrap_or(0.0) as usize;
        let mut values = Vec::with_capacity(length);
        for index in 0..length {
            let key = context.string_rooted(&index.to_string());
            values.push(context.get_property_rooted(value, key)?);
        }
        values
    } else {
        vec![value]
    };
    let mut compiled = Vec::with_capacity(values.len());
    for (index, value) in values.into_iter().enumerate() {
        let name = if is_array {
            format!("options.ignore[{index}]")
        } else {
            "options.ignore".into()
        };
        if let Some(pattern) = context.string_text(value)? {
            if pattern.is_empty() {
                let error = context.type_error_rooted(&format!(
                    "The property '{name}' must be a non-empty string. Received ''"
                ))?;
                super::node_errors_shared_vm::decorate_type_error(
                    context,
                    error,
                    "ERR_INVALID_ARG_VALUE",
                )?;
                super::node_errors_shared_vm::hide_type_error(
                    context,
                    error,
                    "ERR_INVALID_ARG_VALUE",
                )?;
                return Err(context.throw(error));
            }
            let state = context.host_mut().shared_state();
            let api = state
                .borrow()
                .fs_watch
                .api
                .ok_or_else(|| RootedError::host("watch API missing"))?;
            let constructor = get(context, api, "regexpConstructor")?;
            let source = super::path_shared_vm::glob_regex_source(&pattern);
            let source = context.string_rooted(&source);
            let flags = context.string_rooted("u");
            let regexp = context.construct_rooted(constructor, constructor, &[source, flags])?;
            let data = context.object_rooted()?;
            set(context, data, "regexp", regexp)?;
            let basename = context.boolean(!pattern.contains('/'));
            set(context, data, "basename", basename)?;
            let dot = context.boolean(pattern.starts_with('.'));
            set(context, data, "dot", dot)?;
            let native = context.host_function_with_data(
                crate::host::shared_vm::operation("fsWatchIgnorePattern"),
                data,
            )?;
            compiled.push(wrap_matcher(context, native)?);
        } else if context.has_builtin_brand_rooted(value, BuiltinBrand::RegExp)? {
            let native = context.host_function_with_data(
                crate::host::shared_vm::operation("fsWatchIgnoreRegExp"),
                value,
            )?;
            compiled.push(wrap_matcher(context, native)?);
        } else if context.is_callable_rooted(value)? {
            compiled.push(value);
        } else {
            let error = super::util_shared_vm::argument_type_error_with_expected(
                context,
                value,
                &name,
                "one of type string or function or an instance of RegExp",
            )?;
            return Err(super::net_options_shared_vm::validator_error(
                context,
                error,
                "ERR_INVALID_ARG_TYPE",
            )?);
        }
    }
    let array = context.array_rooted(&compiled)?;
    let native = context
        .host_function_with_data(crate::host::shared_vm::operation("fsWatchIgnore"), array)?;
    wrap_matcher(context, native)
}
pub(crate) fn ignore(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let array = context.host_function_data()?;
    let filename = args.first().copied().unwrap_or_else(|| context.undefined());
    let mut index = 0;
    loop {
        let length = get(context, array, "length")?;
        let length = context
            .rooted_value(length)
            .and_then(Value::as_number)
            .unwrap_or(0.0) as usize;
        if index >= length {
            return Ok(context.boolean(false));
        }
        let key = context.string_rooted(&index.to_string());
        let callback = context.get_property_rooted(array, key)?;
        let matched = context.call_rooted(callback, array, &[filename])?;
        if context.truthy_rooted(matched)? {
            return Ok(context.boolean(true));
        }
        index += 1;
    }
}
pub(crate) fn ignore_pattern(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let filename = args.first().copied().unwrap_or_else(|| context.undefined());
    let filename = context.to_string(filename)?;
    let basename = get(context, data, "basename")?;
    let candidate = if context.truthy_rooted(basename)? {
        filename.rsplit('/').next().unwrap_or(&filename)
    } else {
        &filename
    };
    let dot = get(context, data, "dot")?;
    if candidate.starts_with('.') && !context.truthy_rooted(dot)? {
        return Ok(context.boolean(false));
    }
    let regexp = get(context, data, "regexp")?;
    let state = context.host_mut().shared_state();
    let api = state
        .borrow()
        .fs_watch
        .api
        .ok_or_else(|| RootedError::host("watch API missing"))?;
    let exec = get(context, api, "regexpExec")?;
    let candidate = context.string_rooted(candidate);
    let result = context.call_rooted(exec, regexp, &[candidate])?;
    Ok(context.boolean(!nullish(context, result)))
}

pub(crate) fn ignore_regexp(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let regexp = context.host_function_data()?;
    let state = context.host_mut().shared_state();
    let api = state
        .borrow()
        .fs_watch
        .api
        .ok_or_else(|| RootedError::host("watch API missing"))?;
    let exec = get(context, api, "regexpExec")?;
    let filename = args.first().copied().unwrap_or_else(|| context.undefined());
    let result = context.call_rooted(exec, regexp, &[filename])?;
    Ok(context.boolean(!nullish(context, result)))
}

pub(super) fn path(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<(PathBuf, RootId), RootedError> {
    path_argument(context, value, "filename")
}
pub(super) fn path_argument(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<(PathBuf, RootId), RootedError> {
    let value = super::fs_path::validate(context, value, name)?;
    let pathname = super::fs_path::native_path(context, value)?;
    // Watch registries use lexical absolute names, including nonexistent targets.
    // Keep that key separate from the inode-based path used for immediate I/O.
    let pathname = if pathname.is_absolute() {
        pathname
    } else {
        let cwd = context.host_mut().shared_state().borrow().cwd.clone();
        cwd.current_path()
            .map_err(|error| RootedError::host(error.to_string()))?
            .join(pathname)
    };
    Ok((pathname, value))
}

fn watch_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: std::io::Error,
    filename: RootId,
) -> Result<RootedError, RootedError> {
    let errno = error.raw_os_error().map(|errno| -f64::from(errno));
    let code = errno
        .map(super::util_system_error_shared_vm::system_error_name)
        .unwrap_or_else(|| "UNKNOWN".to_owned());
    let message = super::fs_error_codes::strerror(&code);
    let display = context.to_string(filename)?;
    let exception = context.error_rooted(&format!("{code}: {message}, watch '{display}'"))?;
    if let Some(errno) = errno {
        let errno = context.number(errno);
        set(context, exception, "errno", errno)?;
    }
    let syscall = context.string_rooted("watch");
    set(context, exception, "syscall", syscall)?;
    let code = context.string_rooted(&code);
    set(context, exception, "code", code)?;
    set(context, exception, "path", filename)?;
    set(context, exception, "filename", filename)?;
    Ok(context.throw(exception))
}

pub(crate) fn watch(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let mut forwarded = [undefined; 3];
    for (slot, value) in forwarded.iter_mut().zip(args) {
        *slot = *value;
    }
    // SDK watch dispatch precedes overload, option and path validation.
    if let Some(watcher) =
        crate::modules::fs_policy_shared_vm::vfs_read(context, "watch", &forwarded)?
    {
        return Ok(watcher);
    }
    let filename = args.first().copied().unwrap_or_else(|| context.undefined());
    let raw_options = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let listener = if context.is_callable_rooted(raw_options)? {
        raw_options
    } else {
        args.get(2).copied().unwrap_or_else(|| context.undefined())
    };
    let options = clone_options(context, raw_options)?;
    for (name, default) in [
        ("persistent", true),
        ("recursive", false),
        ("throwIfNoEntry", true),
    ] {
        let value = get(context, options, name)?;
        if context
            .rooted_value(value)
            .is_some_and(|value| value.is_undefined())
        {
            let value = context.boolean(default);
            set(context, options, name, value)?;
        }
    }
    let persistent = get(context, options, "persistent")?;
    let referenced = if context
        .rooted_value(persistent)
        .is_some_and(|value| value.is_undefined())
    {
        true
    } else {
        context.truthy_rooted(persistent)?
    };
    let recursive = get(context, options, "recursive")?;
    if context.truthy_rooted(recursive)? {
        let (_, filename) = path(context, filename)?;
        return recursive::watch(context, filename, options, listener);
    }
    let encoding = get(context, options, "encoding")?;
    let encoding = validate_encoding(context, encoding)?;
    let signal = get(context, options, "signal")?;
    super::util_shared_vm::validate_abort_signal(context, signal, "options.signal")?;
    let (pathname, filename) = path(context, filename)?;
    let ignore = get(context, options, "ignore")?;
    let matcher = ignore_matcher(context, ignore)?;
    let state = context.host_mut().shared_state();
    let api = state
        .borrow()
        .fs_watch
        .api
        .ok_or_else(|| RootedError::host("filesystem watcher API missing"))?;
    let constructor = get(context, api, "FSWatcher")?;
    let watcher = context.construct_rooted(constructor, constructor, &[])?;
    set(context, watcher, "_ignoreMatcher", matcher)?;
    let constructor = get(context, api, "FSEvent")?;
    let handle = context.construct_rooted(constructor, constructor, &[])?;
    let undefined = context.undefined();
    set(context, handle, "onchange", undefined)?;
    set(context, watcher, "_handle", handle)?;
    register(
        context,
        handle,
        pathname,
        filename,
        WatchTarget::Watcher(watcher),
        referenced,
        encoding,
        Some(options),
    )?;
    if context.truthy_rooted(listener)? {
        let add = get(context, watcher, "addListener")?;
        let change = context.string_rooted("change");
        context.call_rooted(add, watcher, &[change, listener])?;
    }
    attach_signal(context, watcher, signal)?;
    Ok(watcher)
}

fn register(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
    _pathname: PathBuf,
    filename: RootId,
    target: WatchTarget,
    referenced: bool,
    encoding: String,
    options: Option<RootId>,
) -> Result<(), RootedError> {
    let state = context.host_mut().shared_state();
    let weak = context.host_weak_root(handle)?;
    let id = {
        let mut state = state.borrow_mut();
        let id = state.fs_watch.next_id;
        if id > MAX_WATCH_ID {
            context.release_host_weak_root(weak);
            return Err(RootedError::host("filesystem watcher id space exhausted"));
        }
        state.fs_watch.next_id += 1;
        id
    };
    #[cfg(target_os = "linux")]
    let result = state.borrow_mut().fs_watch.kernel.subscribe(&_pathname);
    #[cfg(not(target_os = "linux"))]
    let result: std::io::Result<WatchDescriptor> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "filesystem watching unsupported",
    ));
    let phase = match result {
        Ok(descriptor) => {
            let root = context.retain(target.root())?;
            let target = target.with_root(root);
            let mut state = state.borrow_mut();
            let group = state
                .fs_watch
                .groups
                .entry(descriptor)
                .or_insert_with(|| Group {
                    #[cfg(target_os = "linux")]
                    basename: _pathname
                        .file_name()
                        .map(|name| name.as_encoded_bytes().to_vec())
                        .unwrap_or_default(),
                    owners: Vec::new(),
                });
            group.owners.push(id);
            Phase::Watching(ActiveWatch {
                descriptor,
                target,
                encoding,
            })
        }
        Err(error) => {
            let throw = if let Some(options) = options {
                get(context, options, "throwIfNoEntry")?
            } else {
                context.boolean(true)
            };
            if error.raw_os_error() == Some(libc::ENOENT)
                && context
                    .rooted_value(throw)
                    .and_then(|value| value.as_bool())
                    == Some(false)
            {
                Phase::Unstarted
            } else {
                context.release_host_weak_root(weak);
                return Err(watch_error(context, error, filename)?);
            }
        }
    };
    state.borrow_mut().fs_watch.handles.insert(
        id,
        Handle {
            weak,
            referenced,
            phase,
        },
    );
    Ok(())
}

pub(super) fn raw_watch(
    context: &mut NativeContext<'_, NodeHost>,
    filename: RootId,
    referenced: bool,
    encoding: String,
    callback: RootId,
) -> Result<RootId, RootedError> {
    let (pathname, filename) = path_argument(context, filename, "path")?;
    let state = context.host_mut().shared_state();
    let api = state
        .borrow()
        .fs_watch
        .api
        .ok_or_else(|| RootedError::host("watch API missing"))?;
    let constructor = get(context, api, "FSEvent")?;
    let handle = context.construct_rooted(constructor, constructor, &[])?;
    set(context, handle, "onchange", callback)?;
    register(
        context,
        handle,
        pathname,
        filename,
        WatchTarget::Handle(handle),
        referenced,
        encoding,
        None,
    )?;
    Ok(handle)
}

fn attach_signal(
    context: &mut NativeContext<'_, NodeHost>,
    watcher: RootId,
    signal: RootId,
) -> Result<(), RootedError> {
    let undefined = context.undefined();
    if !nullish(context, signal) && context.truthy_rooted(signal)? {
        let aborted = get(context, signal, "aborted")?;
        let abort = context
            .host_function_with_data(crate::host::shared_vm::operation("fsWatchAbort"), watcher)?;
        if context.truthy_rooted(aborted)? {
            super::shared_event_loop::queue_next_tick(context, abort, undefined, &[])?;
        } else {
            let add = get(context, signal, "addEventListener")?;
            let event = context.string_rooted("abort");
            let state = context.host_mut().shared_state();
            let events = state
                .borrow()
                .event_targets_api
                .ok_or_else(|| RootedError::host("event API missing"))?;
            let resist = get(context, events, "resistKey")?;
            let options = context.null_object_rooted()?;
            let yes = context.boolean(true);
            context.define_data_property_rooted(options, resist, yes, true, true, true)?;
            context.call_rooted(add, signal, &[event, abort, options])?;
            let data = context.object_rooted()?;
            set(context, data, "signal", signal)?;
            set(context, data, "listener", abort)?;
            let detach = context.host_function_with_data(
                crate::host::shared_vm::operation("fsWatchDetachAbort"),
                data,
            )?;
            let once = get(context, watcher, "once")?;
            let close = context.string_rooted("close");
            context.call_rooted(once, watcher, &[close, detach])?;
        }
    }
    Ok(())
}

fn handle_id(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
) -> Result<u64, RootedError> {
    let state = context.host_mut().shared_state();
    for (id, entry) in &state.borrow().fs_watch.handles {
        if let Some(candidate) = context.resolve_host_weak_root(entry.weak) {
            if context.strict_equal_rooted(candidate, handle)? {
                return Ok(*id);
            }
        }
    }
    let error = context.type_error_rooted("Illegal invocation")?;
    Err(context.throw(error))
}
fn stop(state: &mut State, id: u64) -> Option<RootId> {
    let entry = state.handles.get_mut(&id)?;
    let phase = std::mem::replace(&mut entry.phase, Phase::Stopped);
    let Phase::Watching(active) = phase else {
        entry.phase = phase;
        return None;
    };
    if let Some(group) = state.groups.get_mut(&active.descriptor) {
        group.owners.retain(|owner| *owner != id);
        if group.owners.is_empty() {
            state.groups.remove(&active.descriptor);
            #[cfg(target_os = "linux")]
            state.kernel.unsubscribe(active.descriptor);
        }
    }
    Some(active.target.root())
}
pub(crate) fn handle_close(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let handle = args.first().copied().unwrap_or_else(|| context.undefined());
    let id = handle_id(context, handle)?;
    let state = context.host_mut().shared_state();
    let root = stop(&mut state.borrow_mut().fs_watch, id);
    if let Some(root) = root {
        context.release_root(root);
    }
    Ok(context.undefined())
}
pub(crate) fn handle_ref(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let handle = args.first().copied().unwrap_or_else(|| context.undefined());
    let id = handle_id(context, handle)?;
    let refed = args
        .get(1)
        .copied()
        .map(|value| context.truthy_rooted(value))
        .transpose()?
        .unwrap_or(false);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .fs_watch
        .handles
        .get_mut(&id)
        .expect("resolved handle")
        .referenced = refed;
    Ok(context.undefined())
}
pub(crate) fn handle_has_ref(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let handle = args.first().copied().unwrap_or_else(|| context.undefined());
    let id = handle_id(context, handle)?;
    let referenced = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs_watch
        .handles
        .get(&id)
        .expect("resolved handle")
        .referenced;
    Ok(context.boolean(referenced))
}
pub(crate) fn handle_initialized(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let handle = args.first().copied().unwrap_or_else(|| context.undefined());
    let id = handle_id(context, handle)?;
    let initialized = !matches!(
        context
            .host_mut()
            .shared_state()
            .borrow()
            .fs_watch
            .handles
            .get(&id)
            .expect("resolved handle")
            .phase,
        Phase::Unstarted
    );
    Ok(context.boolean(initialized))
}
pub(crate) fn close(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let watcher = args.first().copied().unwrap_or_else(|| context.undefined());
    let handle = get(context, watcher, "_handle")?;
    if context
        .rooted_value(handle)
        .is_some_and(|value| value.is_null())
    {
        return Ok(context.undefined());
    }
    let id = handle_id(context, handle)?;
    let state = context.host_mut().shared_state();
    if matches!(
        state
            .borrow()
            .fs_watch
            .handles
            .get(&id)
            .expect("resolved handle")
            .phase,
        Phase::Unstarted
    ) {
        return Ok(context.undefined());
    }
    handle_close(context, handle, &[handle])?;
    let null = context.null();
    set(context, watcher, "_handle", null)?;
    let callback = context.host_function(crate::host::shared_vm::operation("fsWatchCloseNT"))?;
    let undefined = context.undefined();
    super::shared_event_loop::queue_next_tick(context, callback, undefined, &[watcher])?;
    Ok(undefined)
}
pub(crate) fn reference(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let watcher = args.first().copied().unwrap_or_else(|| context.undefined());
    let handle = get(context, watcher, "_handle")?;
    if context.truthy_rooted(handle)? {
        let handle = get(context, watcher, "_handle")?;
        let refed = args
            .get(1)
            .copied()
            .unwrap_or_else(|| context.boolean(false));
        let name = if context.truthy_rooted(refed)? {
            "ref"
        } else {
            "unref"
        };
        let method = get(context, handle, name)?;
        context.call_rooted(method, handle, &[])?;
    }
    Ok(watcher)
}
pub(crate) fn abort(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let watcher = context.host_function_data()?;
    let close = get(context, watcher, "close")?;
    context.call_rooted(close, watcher, &[])
}
pub(crate) fn detach_abort(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let signal = get(context, data, "signal")?;
    let listener = get(context, data, "listener")?;
    let remove = get(context, signal, "removeEventListener")?;
    let event = context.string_rooted("abort");
    context.call_rooted(remove, signal, &[event, listener])?;
    Ok(context.undefined())
}
pub(crate) fn close_event(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let watcher = args.first().copied().unwrap_or_else(|| context.undefined());
    let emit = get(context, watcher, "emit")?;
    let event = context.string_rooted("close");
    context.call_rooted(emit, watcher, &[event])
}

pub(crate) fn dispatch(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let state = context.host_mut().shared_state();
    let Some(delivery) = state.borrow_mut().fs_watch.deliveries.pop_front() else {
        return Ok(context.undefined());
    };
    let (target, encoding) = {
        let state = state.borrow();
        let Some(entry) = state.fs_watch.handles.get(&delivery.owner) else {
            return Ok(context.undefined());
        };
        let Phase::Watching(active) = &entry.phase else {
            return Ok(context.undefined());
        };
        (active.target, active.encoding.clone())
    };
    let receiver = context.retain(target.root())?;
    let result = (|| {
        let filename = if encoding == "buffer" {
            let bytes = context.uint8_array_rooted(&delivery.name)?;
            let buffer = crate::host::shared_vm::commonjs::buffer_module(context)?;
            let constructor = get(context, buffer, "Buffer")?;
            let prototype = get(context, constructor, "prototype")?;
            context.set_prototype_rooted(bytes, prototype)?;
            bytes
        } else {
            let units = super::buffer_codec::decode_units(&delivery.name, &encoding);
            context.string_units_rooted(&units)
        };
        let kind = context.string_rooted(match delivery.kind {
            EventKind::Change => "change",
            EventKind::Rename => "rename",
        });
        match target {
            WatchTarget::Handle(_) => {
                let callback = get(context, receiver, "onchange")?;
                let status = context.number(0.0);
                context.call_rooted(callback, receiver, &[status, kind, filename])
            }
            WatchTarget::Watcher(_) => {
                let matcher = get(context, receiver, "_ignoreMatcher")?;
                if !nullish(context, matcher) {
                    let ignored = context.call_rooted(matcher, receiver, &[filename])?;
                    if context.truthy_rooted(ignored)? {
                        return Ok(context.undefined());
                    }
                }
                let emit = get(context, receiver, "emit")?;
                let change = context.string_rooted("change");
                context.call_rooted(emit, receiver, &[change, kind, filename])
            }
        }
    })();
    context.release_root(receiver);
    result
}
pub(crate) fn has_work(state: &State) -> bool {
    state
        .handles
        .values()
        .any(|entry| entry.referenced && matches!(entry.phase, Phase::Watching(_)))
}
pub(crate) fn poll(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<bool, String> {
    let state = runtime.host_mut().shared_state();
    #[cfg(target_os = "linux")]
    if state.borrow().fs_watch.deliveries.is_empty() {
        let event = state
            .borrow_mut()
            .fs_watch
            .kernel
            .next_event()
            .map_err(|error| error.to_string())?;
        if let Some(event) = event {
            let mut state = state.borrow_mut();
            let state = &mut state.fs_watch;
            if let Some(group) = state.groups.get(&event.descriptor) {
                let name = event.name.unwrap_or_else(|| group.basename.clone());
                let owners = group.owners.clone();
                for owner in owners {
                    state.deliveries.push_back(Delivery {
                        owner,
                        kind: event.kind,
                        name: name.clone(),
                    });
                }
            }
        }
    }
    if state.borrow().fs_watch.deliveries.is_empty() {
        return Ok(false);
    }
    let function = state
        .borrow()
        .fs_watch
        .dispatch
        .ok_or("filesystem watch dispatch missing")?;
    let undefined = runtime.root(Value::UNDEFINED);
    let result = runtime.call_rooted(function, undefined, &[]);
    runtime.release_root(undefined);
    match result {
        Ok(root) => {
            runtime.release_root(root);
            Ok(true)
        }
        Err(error) => {
            super::process_shared_vm::route_callback_failure(runtime, program, error)?;
            Ok(true)
        }
    }
}
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let state = runtime.host_mut().shared_state();
    let dead = state
        .borrow()
        .fs_watch
        .handles
        .iter()
        .filter_map(|(id, entry)| (!runtime.host_weak_root_is_live(entry.weak)).then_some(*id))
        .collect::<Vec<_>>();
    for id in dead {
        let root = stop(&mut state.borrow_mut().fs_watch, id);
        if let Some(root) = root {
            runtime.release_root(root);
        }
        if let Some(entry) = state.borrow_mut().fs_watch.handles.remove(&id) {
            runtime.release_host_weak_root(entry.weak);
        }
    }
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let state = runtime.host_mut().shared_state();
    let handles = std::mem::take(&mut state.borrow_mut().fs_watch.handles);
    for entry in handles.into_values() {
        if let Phase::Watching(active) = entry.phase {
            runtime.release_root(active.target.root());
        }
        runtime.release_host_weak_root(entry.weak);
    }
    let mut state = state.borrow_mut();
    state.fs_watch.groups.clear();
    state.fs_watch.deliveries.clear();
    #[cfg(target_os = "linux")]
    state.fs_watch.kernel.reset();
}
