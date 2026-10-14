//! Stat polling owns its resource lifetime separately from the public path cache.
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEFAULT_POLL_INTERVAL_MS: u32 = 5007; // Node retains libev's historical interval.
const MIN_POLL_INTERVAL_MS: u32 = 1; // uv_fs_poll_start normalizes zero to one.
const FACTORY: &str = quench_js_check::checked_js!(
    r#"(Emitter, watch, unwatch, stop, reference) => {
 'use strict';
 function StatWatcher() { Emitter.call(this); this._handle = null; }
 Object.setPrototypeOf(StatWatcher.prototype, Emitter.prototype);
 Object.setPrototypeOf(StatWatcher, Emitter);
 StatWatcher.prototype.start = () => {};
 StatWatcher.prototype.stop = function() { 'use strict'; return stop(this); };
 StatWatcher.prototype.ref = function() { 'use strict'; return reference(this, true); };
 StatWatcher.prototype.unref = function() { 'use strict'; return reference(this, false); };
 return { StatWatcher,
   watchFile: function watchFile(filename, options, listener) { 'use strict'; return watch(filename, options, listener); },
   unwatchFile: function unwatchFile(filename, listener) { 'use strict'; return unwatch(filename, listener); }
 };
}"#
);

enum PollState {
    Initial,
    Present(std::fs::Metadata),
    Missing(i32, Option<std::fs::Metadata>),
}
struct Active {
    watcher: RootId,
    path: PathBuf,
    interval: Duration,
    due: Instant,
    previous: PollState,
    bigint: bool,
}
struct Watcher {
    identity: HostWeakRoot,
    references: usize,
    max_references: usize,
    active: Option<Active>,
}
#[derive(Default)]
pub(crate) struct State {
    api: Option<RootId>,
    dispatch: Option<RootId>,
    cache: HashMap<PathBuf, RootId>,
    watchers: Vec<Watcher>,
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
pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    fs: RootId,
) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let emitter = get(context, global, "__nodeEventEmitter")?;
    let mut arguments = vec![emitter];
    for name in [
        "fsStatWatch",
        "fsStatUnwatch",
        "fsStatWatchStop",
        "fsStatWatchRef",
    ] {
        arguments.push(context.host_function(crate::host::shared_vm::operation(name))?);
    }
    let factory = context.evaluate_script_rooted(FACTORY, "node:fs/stat-watcher-facade.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(factory, undefined, &arguments)?;
    for name in ["watchFile", "unwatchFile"] {
        let function = get(context, api, name)?;
        set(context, fs, name, function)?;
    }
    let api = context.retain(api)?;
    let dispatcher =
        context.host_function(crate::host::shared_vm::operation("fsStatWatchDispatch"))?;
    let dispatcher = context.retain(dispatcher)?;
    let shared = context.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    state.fs_stat_watch.api = Some(api);
    state.fs_stat_watch.dispatch = Some(dispatcher);
    Ok(())
}

fn resolved_path(
    context: &mut NativeContext<'_, NodeHost>,
    filename: RootId,
) -> Result<PathBuf, RootedError> {
    let (path, _) = super::fs_watch_shared_vm::path_argument(context, filename, "path")?;
    // path.resolve is lexical: nonexistent files are valid polling targets.
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if result.file_name().is_some() {
                    result.pop();
                }
            }
            component => result.push(component.as_os_str()),
        }
    }
    Ok(result)
}
fn find(
    context: &mut NativeContext<'_, NodeHost>,
    watcher: RootId,
) -> Result<Option<usize>, RootedError> {
    let shared = context.host_mut().shared_state();
    let identities = shared
        .borrow()
        .fs_stat_watch
        .watchers
        .iter()
        .map(|entry| entry.identity)
        .collect::<Vec<_>>();
    for (index, identity) in identities.into_iter().enumerate() {
        if let Some(value) = context.resolve_host_weak_root(identity) {
            if context.strict_equal_rooted(value, watcher)? {
                return Ok(Some(index));
            }
        }
    }
    Ok(None)
}
fn interval(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<u32, RootedError> {
    let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        let error =
            super::util_shared_vm::argument_type_error(context, value, "interval", "number")?;
        return Err(super::net_options_shared_vm::validator_error(
            context,
            error,
            "ERR_INVALID_ARG_TYPE",
        )?);
    };
    if number.fract() != 0.0 || !number.is_finite() || number < 0.0 || number > f64::from(u32::MAX)
    {
        let detail = super::util_shared_vm::range_error_received(context, value)?;
        let requirement = if number.fract() != 0.0 || !number.is_finite() {
            "an integer".to_owned()
        } else {
            format!(">= 0 && <= {}", u32::MAX)
        };
        let error = context.range_error_rooted(&format!(
            "The value of \"interval\" is out of range. It must be {requirement}. Received {detail}"
        ))?;
        super::node_errors_shared_vm::decorate_range_error(context, error, "ERR_OUT_OF_RANGE")?;
        super::node_errors_shared_vm::hide_range_error(context, error, "ERR_OUT_OF_RANGE")?;
        return Err(context.throw(error));
    }
    Ok((number as u32).max(MIN_POLL_INTERVAL_MS))
}
pub(crate) fn watch(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let filename = args.first().copied().unwrap_or_else(|| context.undefined());
    let path = resolved_path(context, filename)?;
    let supplied = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let mut listener = args.get(2).copied().unwrap_or_else(|| context.undefined());
    let options = context.object_rooted()?;
    let default_interval = context.number(f64::from(DEFAULT_POLL_INTERVAL_MS));
    let persistent = context.boolean(true);
    set(context, options, "interval", default_interval)?;
    set(context, options, "persistent", persistent)?;
    let object = context.is_object_rooted(supplied)? && !context.is_callable_rooted(supplied)?;
    if object {
        for key in context.enumerable_own_keys_rooted(supplied, true, true)? {
            let value = context.get_property_rooted(supplied, key)?;
            context.define_data_property_rooted(options, key, value, true, true, true)?;
        }
    } else {
        listener = supplied;
    }
    if !context.is_callable_rooted(listener)? {
        let error =
            super::util_shared_vm::argument_type_error(context, listener, "listener", "function")?;
        return Err(super::net_options_shared_vm::validator_error(
            context,
            error,
            "ERR_INVALID_ARG_TYPE",
        )?);
    }
    let shared = context.host_mut().shared_state();
    let cached = shared.borrow().fs_stat_watch.cache.get(&path).copied();
    let watcher = if let Some(watcher) = cached {
        let index = find(context, watcher)?
            .ok_or_else(|| RootedError::host("cached stat watcher lost identity"))?;
        let mut state = shared.borrow_mut();
        let entry = &mut state.fs_stat_watch.watchers[index];
        entry.references += 1;
        entry.max_references += 1;
        watcher
    } else {
        let bigint = get(context, options, "bigint")?;
        let bigint = context
            .rooted_value(bigint)
            .and_then(|value| value.as_bool())
            == Some(true);
        let persistent = get(context, options, "persistent")?;
        let persistent = context.truthy_rooted(persistent)?;
        let interval_value = get(context, options, "interval")?;
        let interval = Duration::from_millis(u64::from(interval(context, interval_value)?));
        let api = shared
            .borrow()
            .fs_stat_watch
            .api
            .ok_or_else(|| RootedError::host("stat watcher API missing"))?;
        let constructor = get(context, api, "StatWatcher")?;
        let watcher = context.construct_rooted(constructor, constructor, &[])?;
        let handle = context.object_rooted()?;
        set(context, watcher, "_handle", handle)?;
        let identity = context.host_weak_root(watcher)?;
        let watcher = context.retain(watcher)?;
        let cache_root = context.retain(watcher)?;
        let mut state = shared.borrow_mut();
        state.fs_stat_watch.watchers.push(Watcher {
            identity,
            references: usize::from(persistent),
            max_references: 1,
            active: Some(Active {
                watcher,
                path: path.clone(),
                interval,
                due: Instant::now(),
                previous: PollState::Initial,
                bigint,
            }),
        });
        state.fs_stat_watch.cache.insert(path, cache_root);
        watcher
    };
    let add = get(context, watcher, "addListener")?;
    let change = context.string_rooted("change");
    context.call_rooted(add, watcher, &[change, listener])?;
    Ok(watcher)
}
pub(crate) fn reference(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let watcher = args.first().copied().unwrap_or_else(|| context.undefined());
    let reference = args
        .get(1)
        .copied()
        .unwrap_or_else(|| context.boolean(false));
    let reference = context.truthy_rooted(reference)?;
    if let Some(index) = find(context, watcher)? {
        let shared = context.host_mut().shared_state();
        let mut state = shared.borrow_mut();
        let entry = &mut state.fs_stat_watch.watchers[index];
        if entry.active.is_some() {
            if reference && entry.references != entry.max_references {
                entry.references += 1;
            } else if !reference && entry.references != 0 {
                entry.references -= 1;
            }
        }
    }
    Ok(watcher)
}
pub(crate) fn stop(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let watcher = args.first().copied().unwrap_or_else(|| context.undefined());
    if let Some(index) = find(context, watcher)? {
        let shared = context.host_mut().shared_state();
        let active = shared.borrow_mut().fs_stat_watch.watchers[index]
            .active
            .take();
        if let Some(active) = active {
            let callback =
                context.host_function(crate::host::shared_vm::operation("fsStatWatchStopNT"))?;
            let undefined = context.undefined();
            super::shared_event_loop::queue_next_tick(context, callback, undefined, &[watcher])?;
            let null = context.null();
            set(context, watcher, "_handle", null)?;
            context.release_root(active.watcher);
        }
    }
    Ok(context.undefined())
}
pub(crate) fn stop_event(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let watcher = args.first().copied().unwrap_or_else(|| context.undefined());
    let emit = get(context, watcher, "emit")?;
    let stop = context.string_rooted("stop");
    context.call_rooted(emit, watcher, &[stop])
}
pub(crate) fn unwatch(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let filename = args.first().copied().unwrap_or_else(|| context.undefined());
    let path = resolved_path(context, filename)?;
    let shared = context.host_mut().shared_state();
    let Some(watcher) = shared.borrow().fs_stat_watch.cache.get(&path).copied() else {
        return Ok(context.undefined());
    };
    let listener = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let index = find(context, watcher)?
        .ok_or_else(|| RootedError::host("cached stat watcher lost identity"))?;
    let count = get(context, watcher, "listenerCount")?;
    let change = context.string_rooted("change");
    if context.is_callable_rooted(listener)? {
        let before = context.call_rooted(count, watcher, &[change])?;
        let before = context
            .rooted_value(before)
            .and_then(|value| value.as_number())
            .unwrap_or(0.0);
        let remove = get(context, watcher, "removeListener")?;
        context.call_rooted(remove, watcher, &[change, listener])?;
        let after = context.call_rooted(count, watcher, &[change])?;
        let after = context
            .rooted_value(after)
            .and_then(|value| value.as_number())
            .unwrap_or(0.0);
        if after < before {
            let mut state = shared.borrow_mut();
            let entry = &mut state.fs_stat_watch.watchers[index];
            entry.max_references -= 1;
            if entry.active.is_some() && entry.references != 0 {
                entry.references -= 1;
            }
        }
    } else {
        let remove = get(context, watcher, "removeAllListeners")?;
        context.call_rooted(remove, watcher, &[change])?;
        let mut state = shared.borrow_mut();
        let entry = &mut state.fs_stat_watch.watchers[index];
        entry.references = 0;
        entry.max_references = 0;
    }
    let remaining = context.call_rooted(count, watcher, &[change])?;
    if context
        .rooted_value(remaining)
        .and_then(|value| value.as_number())
        == Some(0.0)
    {
        let stop = get(context, watcher, "stop")?;
        context.call_rooted(stop, watcher, &[])?;
        let retained = shared.borrow_mut().fs_stat_watch.cache.remove(&path);
        if let Some(retained) = retained {
            context.release_root(retained);
        }
    }
    Ok(context.undefined())
}

pub(crate) fn has_work(state: &State) -> bool {
    state.watchers.iter().any(|entry| {
        entry.active.as_ref().is_some_and(|active| {
            entry.references != 0 || matches!(active.previous, PollState::Initial)
        })
    })
}
pub(crate) fn poll(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<bool, String> {
    let shared = runtime.host_mut().shared_state();
    let due = shared.borrow().fs_stat_watch.watchers.iter().any(|entry| {
        entry
            .active
            .as_ref()
            .is_some_and(|active| active.due <= Instant::now())
    });
    if !due {
        return Ok(false);
    }
    let function = shared
        .borrow()
        .fs_stat_watch
        .dispatch
        .ok_or("stat watch dispatcher missing")?;
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

pub(crate) fn dispatch(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let now = Instant::now();
    let ready = shared
        .borrow()
        .fs_stat_watch
        .watchers
        .iter()
        .position(|entry| {
            entry
                .active
                .as_ref()
                .is_some_and(|active| active.due <= now)
        });
    let Some(index) = ready else {
        return Ok(context.undefined());
    };
    let notification = {
        let mut state = shared.borrow_mut();
        let active = state.fs_stat_watch.watchers[index]
            .active
            .as_mut()
            .expect("due active watcher");
        active.due = now + active.interval;
        let current = std::fs::metadata(&active.path);
        let previous = std::mem::replace(&mut active.previous, PollState::Initial);
        let (next, change) = match current {
            Ok(metadata) => {
                let change = match &previous {
                    PollState::Initial => false,
                    PollState::Present(old) => !same_stat(old, &metadata),
                    PollState::Missing(..) => true,
                };
                let old = match previous {
                    PollState::Present(old) => Some(old),
                    PollState::Missing(_, old) => old,
                    _ => None,
                };
                let notification =
                    change.then(|| (active.watcher, active.bigint, Some(metadata.clone()), old));
                (PollState::Present(metadata), notification)
            }
            Err(error) => {
                let errno = error.raw_os_error().unwrap_or(libc::EIO);
                let change = !matches!(&previous, PollState::Missing(old, _) if *old == errno)
                    && !(matches!(&previous, PollState::Initial) && errno == libc::EPERM);
                let old = match previous {
                    PollState::Present(old) => Some(old),
                    PollState::Missing(_, old) => old,
                    PollState::Initial => None,
                };
                let notification =
                    change.then(|| (active.watcher, active.bigint, None, old.clone()));
                (PollState::Missing(errno, old), notification)
            }
        };
        active.previous = next;
        change
    };
    if let Some((watcher, bigint, current, previous)) = notification {
        let watcher = context.retain(watcher)?;
        let result = (|| {
            let current =
                super::fs_shared_vm::stat::metadata_stats(context, current.as_ref(), bigint)?;
            let previous =
                super::fs_shared_vm::stat::metadata_stats(context, previous.as_ref(), bigint)?;
            let emit = get(context, watcher, "emit")?;
            let change = context.string_rooted("change");
            context.call_rooted(emit, watcher, &[change, current, previous])
        })();
        context.release_root(watcher);
        return result;
    }
    Ok(context.undefined())
}
#[cfg(unix)]
fn same_stat(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.created().ok() == b.created().ok()
        && a.len() == b.len()
        && a.mode() == b.mode()
        && a.uid() == b.uid()
        && a.gid() == b.gid()
        && a.ino() == b.ino()
        && a.dev() == b.dev()
}
#[cfg(not(unix))]
fn same_stat(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    a.modified().ok() == b.modified().ok()
        && a.created().ok() == b.created().ok()
        && a.len() == b.len()
        && a.permissions() == b.permissions()
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let watchers = std::mem::take(&mut shared.borrow_mut().fs_stat_watch.watchers);
    let cache = std::mem::take(&mut shared.borrow_mut().fs_stat_watch.cache);
    for entry in watchers {
        if let Some(active) = entry.active {
            runtime.release_root(active.watcher);
        }
        runtime.release_host_weak_root(entry.identity);
    }
    for root in cache.into_values() {
        runtime.release_root(root);
    }
}

pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let dead = shared
        .borrow()
        .fs_stat_watch
        .watchers
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (!runtime.host_weak_root_is_live(entry.identity)).then_some(index)
        })
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let entry = shared.borrow_mut().fs_stat_watch.watchers.remove(index);
        debug_assert!(
            entry.active.is_none(),
            "active stat watch retains its watcher"
        );
        runtime.release_host_weak_root(entry.identity);
    }
}
