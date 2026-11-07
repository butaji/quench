//! `node:v8` (lib/v8.js and its bindings src/node_v8.cc, src/heap_utils.cc,
//! src/node_serdes.cc). Heap statistics report Quench's heap: one space, sized by the
//! runtime's heap accounting, under Node's default heap limit. A heap snapshot is V8's
//! format holding only its root; the GC profiler and coverage report nothing collected.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;


/// V8's space names, in `kHeapSpaces` order; Quench's single heap reports as old space.
const HEAP_SPACES: [&str; 11] = [
    "read_only_space",
    "new_space",
    "old_space",
    "code_space",
    "shared_space",
    "trusted_space",
    "new_large_object_space",
    "large_object_space",
    "code_large_object_space",
    "shared_large_object_space",
    "trusted_large_object_space",
];
const HEAP_SPACE: &str = "old_space";

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
/// V8's heap sizing defaults for 64-bit hosts (src/heap/heap.cc): the old generation is
/// a quarter of physical memory times the pointer multiplier, between the minimum and a
/// maximum doubled on hosts with at least 15 GiB; three semi-space-sized young regions
/// sit beside it.
const HEAP_LIMIT_MULTIPLIER: u64 = (u64::BITS / 8 / 4) as u64;
const PHYSICAL_MEMORY_TO_OLD_GENERATION_RATIO: u64 = 4;
const DEFAULT_MAX_HEAP_SIZE: u64 = 1024 * MIB * HEAP_LIMIT_MULTIPLIER;
const DEFAULT_MIN_HEAP_SIZE: u64 = 128 * MIB * HEAP_LIMIT_MULTIPLIER;
const LARGE_HOST_MEMORY_GIB: u64 = 15;
const NORMAL_PAGE_SIZE: u64 = 256 * 1024;
const MAX_SEMI_SPACE_SIZE: u64 = 32 * MIB;
const YOUNG_GENERATION_SEMI_SPACES: u64 = 3;
const MAX_OLD_SPACE_SIZE_OPTION: &str = "--max-old-space-size=";

/// `heap_size_limit`: `--max-old-space-size` or V8's physical-memory default for the old
/// generation, plus the young generation's reservation.
fn heap_size_limit(exec_argv: &[String]) -> u64 {
    let configured = exec_argv
        .iter()
        .rev()
        .find_map(|argument| argument.strip_prefix(MAX_OLD_SPACE_SIZE_OPTION))
        .and_then(|megabytes| megabytes.parse::<u64>().ok())
        .map(|megabytes| megabytes * MIB);
    let old_generation = configured.unwrap_or_else(|| {
        let physical = crate::modules::os_facts::total_memory_bytes();
        let mut maximum = DEFAULT_MAX_HEAP_SIZE;
        if physical / GIB >= LARGE_HOST_MEMORY_GIB {
            maximum *= 2;
        }
        let proportional = physical / PHYSICAL_MEMORY_TO_OLD_GENERATION_RATIO * HEAP_LIMIT_MULTIPLIER;
        proportional.min(maximum).max(DEFAULT_MIN_HEAP_SIZE).div_ceil(NORMAL_PAGE_SIZE) * NORMAL_PAGE_SIZE
    });
    old_generation + YOUNG_GENERATION_SEMI_SPACES * MAX_SEMI_SPACE_SIZE
}

/// `[used, reserved, limit]` bytes.
fn heap_sizes(c: &mut Context<'_>) -> [f64; 3] {
    let composition = c.heap_memory_composition();
    let limit = heap_size_limit(&c.host_mut().shared_state().borrow().exec_argv);
    [composition.used_bytes() as f64, composition.reserved_bytes() as f64, limit as f64]
}

/// `getHeapStatistics()`'s members, in lib/v8.js order (`heap_statistics` values).
const HEAP_STATISTICS: [&str; 14] = [
    "total_heap_size",
    "total_heap_size_executable",
    "total_physical_size",
    "total_available_size",
    "used_heap_size",
    "heap_size_limit",
    "malloced_memory",
    "peak_malloced_memory",
    "does_zap_garbage",
    "number_of_native_contexts",
    "number_of_detached_contexts",
    "total_global_handles_size",
    "used_global_handles_size",
    "external_memory",
];
const HEAP_SPACE_STATISTICS: [&str; 4] = ["space_size", "space_used_size", "space_available_size", "physical_space_size"];
/// `getHeapCodeStatistics()`'s members; Quench keeps no code outside its heap, so each is 0.
const HEAP_CODE_STATISTICS: [&str; 4] =
    ["code_and_metadata_size", "bytecode_and_metadata_size", "external_script_source_size", "cpu_profiler_metadata_size"];
/// `getCppHeapStatistics()`: Quench has no C++ heap, so every size is 0.
const CPP_HEAP_SIZES: [&str; 3] = ["committed_size_bytes", "resident_size_bytes", "used_size_bytes"];
const CPP_DETAIL_LEVELS: [&str; 2] = ["brief", "detailed"];
const DEFAULT_CPP_DETAIL_LEVEL: &str = "detailed";
const QUERY_FORMATS: [&str; 2] = ["count", "summary"];
const SUMMARY_FORMAT: &str = "summary";
const QUERY_OBJECTS_WARNING: &str = "v8.queryObjects() is an experimental feature and might change at any time";
const NOT_BUILDING_SNAPSHOT: &str = "Operation cannot be invoked when not building startup snapshot";

/// An empty heap snapshot in V8's format: Quench's heap is not walked into one, so the
/// snapshot holds only V8's synthetic root.
const EMPTY_HEAP_SNAPSHOT: &str = concat!(
    r#"{"snapshot":{"meta":{"node_fields":["type","name","id","self_size","edge_count","detachedness"],"#,
    r#""node_types":[["hidden","array","string","object","code","closure","regexp","number","native","synthetic","#,
    r#""concatenated string","sliced string","symbol","bigint","object shape"],"string","number","number","number","number"],"#,
    r#""edge_fields":["type","name_or_index","to_node"],"edge_types":[["context","element","property","internal","hidden","#,
    r#""shortcut","weak"],"string_or_number","node"],"trace_function_info_fields":["function_id","name","script_name","#,
    r#""script_id","line","column"],"trace_node_fields":["id","function_info_index","count","size","children"],"#,
    r#""sample_fields":["timestamp_us","last_assigned_id"],"location_fields":["object_index","script_id","line","column"]},"#,
    r#""node_count":1,"edge_count":0,"trace_function_count":0},"nodes":[9,0,1,0,0,0],"edges":[],"#,
    r#""trace_function_infos":[],"trace_tree":[],"samples":[],"locations":[],"strings":[""]}"#,
);
const HEAP_SNAPSHOT_EXTENSION: &str = "heapsnapshot";
/// `DiagnosticFilename`'s date and time fields and sequence width.
const SNAPSHOT_DATE_FORMAT: &str = "%Y%m%d";
const SNAPSHOT_TIME_FORMAT: &str = "%H%M%S";
const SNAPSHOT_SEQUENCE_WIDTH: usize = 3;

/// The promise hook kinds, in `promiseHooks`' order: `(kind, register function, hook argument)`.
const PROMISE_HOOKS: [(&str, &str, &str); 4] =
    [("init", "onInit", "initHook"), ("before", "onBefore", "beforeHook"), ("after", "onAfter", "afterHook"), ("settled", "onSettled", "settledHook")];

/// A GC profiler's lifecycle: it starts once and stops once.
mod profiler {
    pub(super) const INITIALIZED: f64 = 0.0;
    pub(super) const STARTED: f64 = 1.0;
    pub(super) const STOPPED: f64 = 2.0;
    pub(crate) use crate::modules::keys::STATE;
    pub(super) const START_TIME: &str = "startTime";
    /// The profile format `stop()` reports.
    pub(super) const VERSION: f64 = 1.0;
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}

fn put_number(c: &mut Context<'_>, object: RootId, name: &str, value: f64) -> Result<(), RootedError> {
    let value = c.number(value);
    put(c, object, name, value)
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn validate_object(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    let object = c.value_type_rooted(value) == Some(quench_runtime::ValueType::Object) && !c.is_array_rooted(value)?;
    if !object {
        return Err(super::util_shared_vm::argument_type_error(c, value, name, "Object")?);
    }
    Ok(())
}

fn validate_function(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if !c.is_callable_rooted(value)? {
        return Err(super::util_shared_vm::argument_type_error(c, value, name, "function")?);
    }
    Ok(())
}

fn validate_boolean_option(c: &mut Context<'_>, options: RootId, name: &str) -> Result<(), RootedError> {
    let value = get(c, options, name)?;
    if !is_undefined(c, value) && c.rooted_value(value).and_then(|value| value.as_bool()).is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, value, &format!("options.{name}"), "boolean")?);
    }
    Ok(())
}

/// `emitExperimentalWarning(feature)`: once per environment, which marks the feature in
/// its builtin cache.
pub(crate) fn warn_experimental_once(c: &mut Context<'_>, feature: &'static str, message: &str) -> Result<(), RootedError> {
    let mut first = false;
    crate::host::shared_vm::commonjs::cached_native_binding(c, feature, |c| {
        first = true;
        Ok(c.undefined())
    })?;
    if !first {
        return Ok(());
    }
    let process = c.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process is not initialized"))?;
    let message = c.string_rooted(message);
    let kind = c.string_rooted("ExperimentalWarning");
    super::stream_shared_vm::call_method(c, process, "emitWarning", &[message, kind])?;
    Ok(())
}

fn coded_error(c: &mut Context<'_>, message: &str, code: &'static str) -> RootedError {
    let error = c.error_rooted(message).and_then(|error| {
        super::node_errors_shared_vm::decorate_error(c, error, code)?;
        Ok(error)
    });
    match error {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

/// `getHeapStatistics()`.
pub(crate) fn get_heap_statistics(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let values = heap_statistics(c);
    let result = c.object_rooted()?;
    for (name, value) in HEAP_STATISTICS.iter().zip(values) {
        put_number(c, result, name, value)?;
    }
    Ok(result)
}

/// `getHeapSpaceStatistics()`: one record per V8 space.
pub(crate) fn get_heap_space_statistics(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let mut spaces = Vec::with_capacity(HEAP_SPACES.len());
    for name in HEAP_SPACES {
        let values = heap_space_statistics(c, name);
        let space = c.object_rooted()?;
        let name = c.string_rooted(name);
        put(c, space, "space_name", name)?;
        for (field, value) in HEAP_SPACE_STATISTICS.iter().zip(values) {
            put_number(c, space, field, value)?;
        }
        spaces.push(space);
    }
    c.array_rooted(&spaces)
}

/// `getHeapCodeStatistics()`.
pub(crate) fn get_heap_code_statistics(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let result = c.object_rooted()?;
    for name in HEAP_CODE_STATISTICS {
        put_number(c, result, name, 0.0)?;
    }
    Ok(result)
}

/// `getCppHeapStatistics(type = 'detailed')`.
pub(crate) fn get_cpp_heap_statistics(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let level = argument(c, args, 0);
    let level = if is_undefined(c, level) { c.string_rooted(DEFAULT_CPP_DETAIL_LEVEL) } else { level };
    let text = c.string_text(level)?;
    let Some(level) = text.as_deref().and_then(|text| CPP_DETAIL_LEVELS.iter().find(|candidate| **candidate == text)) else {
        let listed = CPP_DETAIL_LEVELS.map(|level| format!("'{level}'")).join(", ");
        return Err(super::util_shared_vm::argument_value_error(c, level, "type", &format!("must be one of: {listed}"))?);
    };
    let result = c.object_rooted()?;
    for name in CPP_HEAP_SIZES {
        put_number(c, result, name, 0.0)?;
    }
    for name in ["space_statistics", "type_names"] {
        let empty = c.array_rooted(&[])?;
        put(c, result, name, empty)?;
    }
    let level = c.string_rooted(level);
    put(c, result, "detail_level", level)?;
    Ok(result)
}

/// `setFlagsFromString(flags)`: Quench has no V8 flags to set.
pub(crate) fn set_flags_from_string(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let flags = argument(c, args, 0);
    if c.string_text(flags)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, flags, "flags", "string")?);
    }
    Ok(c.undefined())
}

/// `setHeapSnapshotNearHeapLimit(limit)`: Quench writes no snapshot near its limit.
pub(crate) fn set_heap_snapshot_near_heap_limit(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let limit = argument(c, args, 0);
    let maximum = c.number(f64::from(u32::MAX));
    super::util_shared_vm::validate_integer(c, limit, "limit", 1.0, maximum)?;
    Ok(c.undefined())
}

/// `takeCoverage()` and `stopCoverage()`: Quench writes no V8 coverage.
pub(crate) fn coverage(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}

/// `getHeapSnapshotOptions(options)`'s validation.
fn validate_snapshot_options(c: &mut Context<'_>, options: RootId) -> Result<(), RootedError> {
    if is_undefined(c, options) {
        return Ok(());
    }
    validate_object(c, options, "options")?;
    validate_boolean_option(c, options, "exposeInternals")?;
    validate_boolean_option(c, options, "exposeNumericValues")
}

/// `getHeapSnapshot(options)`: a Readable of the snapshot's JSON.
pub(crate) fn get_heap_snapshot(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let options = argument(c, args, 0);
    validate_snapshot_options(c, options)?;
    let stream = crate::host::shared_vm::commonjs::stream_module(c)?;
    let readable = get(c, stream, "Readable")?;
    let stream_options = c.object_rooted()?;
    let read = c.host_function_named(crate::host::shared_vm::operation!("streamNop"), "read")?;
    put(c, stream_options, "read", read)?;
    let snapshot = c.construct_rooted(readable, readable, &[stream_options])?;
    let chunk = super::buffer_shared_vm::from_bytes(c, EMPTY_HEAP_SNAPSHOT.as_bytes())?;
    let end = c.null();
    for value in [chunk, end] {
        super::stream_shared_vm::call_method(c, snapshot, "push", &[value])?;
    }
    Ok(snapshot)
}

/// Node's `DiagnosticFilename`: `Heap.<date>.<time>.<pid>.<thread>.<sequence>.heapsnapshot`.
/// The sequence is the process's, as Node's is.
fn snapshot_filename(c: &mut Context<'_>) -> String {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let thread = c.host_mut().shared_state().borrow().messaging.context.as_ref().map_or(0, |context| context.thread_id);
    let now = chrono::Local::now();
    format!(
        "Heap.{}.{}.{}.{thread}.{sequence:0width$}.{HEAP_SNAPSHOT_EXTENSION}",
        now.format(SNAPSHOT_DATE_FORMAT),
        now.format(SNAPSHOT_TIME_FORMAT),
        std::process::id(),
        width = SNAPSHOT_SEQUENCE_WIDTH,
    )
}

/// `getValidatedPath(path)` for a string, Buffer or file URL.
fn validated_path(c: &mut Context<'_>, path: RootId) -> Result<String, RootedError> {
    if let Some(text) = c.string_text(path)? {
        return Ok(text);
    }
    if let Some(bytes) = super::serdes_shared_vm::view_bytes(c, path)? {
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    let url = crate::host::shared_vm::commonjs::url_module(c)?;
    let url_class = get(c, url, "URL")?;
    if c.is_object_rooted(path)? && c.instance_of_rooted(path, url_class)? {
        let to_path = get(c, url, "fileURLToPath")?;
        let path = c.call_rooted(to_path, url, &[path])?;
        return c.to_string(path);
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(c, path, "path", "of type string or an instance of Buffer or URL")?)
}

/// `writeHeapSnapshot(filename, options)`: write the snapshot and return its file name.
pub(crate) fn write_heap_snapshot(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let filename = argument(c, args, 0);
    let filename = if is_undefined(c, filename) { None } else { Some(validated_path(c, filename)?) };
    let options = argument(c, args, 1);
    validate_snapshot_options(c, options)?;
    let filename = match filename {
        Some(filename) => filename,
        None => snapshot_filename(c),
    };
    if let Err(error) = std::fs::write(&filename, EMPTY_HEAP_SNAPSHOT) {
        let message = format!("{error}, open '{filename}'");
        return Err(coded_error(c, &message, "ERR_HEAP_SNAPSHOT_FAILED"));
    }
    Ok(c.string_rooted(&filename))
}

/// `queryObjects(constructor, options)`: how many live objects `constructor.prototype`
/// heads the prototype of, or their summaries.
pub(crate) fn query_objects_public(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let constructor = argument(c, args, 0);
    validate_function(c, constructor, "constructor")?;
    let options = argument(c, args, 1);
    let format = if is_undefined(c, options) {
        c.undefined()
    } else {
        validate_object(c, options, "options")?;
        get(c, options, "format")?
    };
    let format = if c.truthy_rooted(format)? { format } else { c.string_rooted(QUERY_FORMATS[0]) };
    let text = c.string_text(format)?;
    let Some(format) = text.as_deref().and_then(|text| QUERY_FORMATS.iter().find(|candidate| **candidate == text)) else {
        return Err(super::util_shared_vm::argument_value_error(c, format, "options.format", "is invalid")?);
    };
    warn_experimental_once(c, "\0warned:v8.queryObjects", QUERY_OBJECTS_WARNING)?;
    let prototype = get(c, constructor, "prototype")?;
    let objects = query_objects(c, receiver, &[prototype])?;
    let objects = c.array_like_values_rooted(objects)?;
    if *format != SUMMARY_FORMAT {
        return Ok(c.number(objects.len() as f64));
    }
    let mut summaries = Vec::with_capacity(objects.len());
    for object in objects {
        let options = super::console_format_shared_vm::Options { depth: Some(0.0), ..Default::default() };
        let summary = super::console_format_shared_vm::inspect(c, object, options)?;
        summaries.push(c.string_rooted(&summary));
    }
    c.array_rooted(&summaries)
}

/// `startupSnapshot.isBuildingSnapshot()`: Node reads its flag cell, whose 0 means no.
/// Quench's startup snapshots are its own, never built from a user program.
pub(crate) fn is_building_snapshot(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    const NOT_BUILDING: f64 = 0.0;
    Ok(c.number(NOT_BUILDING))
}

/// The other `startupSnapshot` methods, which only a snapshot build may call.
pub(crate) fn not_building_snapshot(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Err(coded_error(c, NOT_BUILDING_SNAPSHOT, "ERR_NOT_BUILDING_SNAPSHOT"))
}

fn object_with_methods(c: &mut Context<'_>, methods: &[(&str, &str)]) -> Result<RootId, RootedError> {
    let object = c.object_rooted()?;
    for (name, operation) in methods {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        put(c, object, name, function)?;
    }
    Ok(object)
}

/// The registry `promise_hooks_shared_vm` emits to: each kind's hooks, and `emit`.
fn promise_hook_registry(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0builtin:v8-promise-hooks", |c| {
        let registry = c.null_object_rooted()?;
        for (kind, _, _) in PROMISE_HOOKS {
            let hooks = c.array_rooted(&[])?;
            put(c, registry, kind, hooks)?;
        }
        let emit = c.host_function_named_with_data(crate::host::shared_vm::operation!("v8PromiseHooksEmit"), "emit", registry)?;
        put(c, registry, "emit", emit)?;
        Ok(registry)
    })
}

fn hook_kind(c: &mut Context<'_>) -> Result<usize, RootedError> {
    let kind = c.host_function_data()?;
    let kind = c.to_number(kind)? as usize;
    Ok(kind.min(PROMISE_HOOKS.len() - 1))
}

/// `promiseHooks.onInit(hook)` and its siblings: subscribe `hook`, returning its stop.
pub(crate) fn add_promise_hook(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let kind = hook_kind(c)?;
    let (name, _, argument_name) = PROMISE_HOOKS[kind];
    let hook = argument(c, args, 0);
    validate_function(c, hook, argument_name)?;
    let registry = promise_hook_registry(c)?;
    let hooks = get(c, registry, name)?;
    let mut list = c.array_like_values_rooted(hooks)?;
    list.push(hook);
    let hooks = c.array_rooted(&list)?;
    put(c, registry, name, hooks)?;
    super::promise_hooks_shared_vm::set_registry(c, registry, &[registry])?;
    let kind = c.number(kind as f64);
    let data = c.array_rooted(&[kind, hook])?;
    c.host_function_named_with_data(crate::host::shared_vm::operation!("v8PromiseHookStop"), "stop", data)
}

/// A hook's stop: unsubscribe it; with no hook left, promises are no longer observed.
pub(crate) fn stop_promise_hook(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let [kind, hook] = match c.array_like_values_rooted(data)?[..] {
        [kind, hook] => [kind, hook],
        _ => return Err(RootedError::host("promise hook stop data")),
    };
    let kind = (c.to_number(kind)? as usize).min(PROMISE_HOOKS.len() - 1);
    let registry = promise_hook_registry(c)?;
    let name = PROMISE_HOOKS[kind].0;
    let hooks = get(c, registry, name)?;
    let mut list = c.array_like_values_rooted(hooks)?;
    let mut position = None;
    for (index, candidate) in list.iter().enumerate() {
        if c.strict_equal_rooted(*candidate, hook)? {
            position = Some(index);
            break;
        }
    }
    if let Some(index) = position {
        list.remove(index);
    }
    let hooks = c.array_rooted(&list)?;
    put(c, registry, name, hooks)?;
    let mut any = false;
    for (kind, _, _) in PROMISE_HOOKS {
        let hooks = get(c, registry, kind)?;
        any |= !c.array_like_values_rooted(hooks)?.is_empty();
    }
    if !any {
        super::promise_hooks_shared_vm::clear_registry(c);
    }
    Ok(c.undefined())
}

/// The registry's `emit(kind, ...args)`: run that kind's hooks.
pub(crate) fn emit_promise_hooks(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let registry = c.host_function_data()?;
    let kind = argument(c, args, 0);
    let kind = c.to_string(kind)?;
    let hooks = get(c, registry, &kind)?;
    let undefined = c.undefined();
    for hook in c.array_like_values_rooted(hooks)? {
        c.call_rooted(hook, undefined, args.get(1..).unwrap_or_default())?;
    }
    Ok(undefined)
}

/// `promiseHooks.createHook({ init, before, after, settled })`: one stop for them all.
pub(crate) fn create_promise_hook(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let hooks = argument(c, args, 0);
    let hooks = if is_undefined(c, hooks) { c.object_rooted()? } else { hooks };
    let mut stops = Vec::new();
    for (kind, (name, _, _)) in PROMISE_HOOKS.iter().enumerate() {
        let hook = get(c, hooks, name)?;
        if c.truthy_rooted(hook)? {
            let kind = c.number(kind as f64);
            let add = c.host_function_named_with_data(crate::host::shared_vm::operation!("v8AddPromiseHook"), name, kind)?;
            stops.push(c.call_rooted(add, receiver, &[hook])?);
        }
    }
    let stops = c.array_rooted(&stops)?;
    c.host_function_named_with_data(crate::host::shared_vm::operation!("v8PromiseHooksStopAll"), "", stops)
}

pub(crate) fn stop_all_promise_hooks(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let stops = c.host_function_data()?;
    let undefined = c.undefined();
    for stop in c.array_like_values_rooted(stops)? {
        c.call_rooted(stop, undefined, &[])?;
    }
    Ok(undefined)
}

fn promise_hooks(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let object = object_with_methods(c, &[("createHook", "v8CreatePromiseHook")])?;
    for (kind, (_, name, _)) in PROMISE_HOOKS.iter().enumerate() {
        let kind = c.number(kind as f64);
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation!("v8AddPromiseHook"), name, kind)?;
        put(c, object, name, function)?;
    }
    Ok(object)
}

fn profiler_key(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0builtin:v8-gc-profiler", |c| {
        Ok(c.private_symbol_rooted(Some("GCProfiler state")))
    })
}

fn profiler_state(c: &mut Context<'_>, receiver: RootId) -> Result<RootId, RootedError> {
    let key = profiler_key(c)?;
    let state = if c.is_object_rooted(receiver)? { c.get_property_rooted(receiver, key)? } else { c.undefined() };
    if is_undefined(c, state) {
        let error = c.type_error_rooted("Illegal invocation")?;
        return Err(c.throw(error));
    }
    Ok(state)
}

/// `new GCProfiler()`.
pub(crate) fn gc_profiler(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        let error = c.type_error_rooted("Class constructor GCProfiler cannot be invoked without 'new'")?;
        return Err(c.throw(error));
    }
    let key = profiler_key(c)?;
    let state = c.null_object_rooted()?;
    put_number(c, state, profiler::STATE, profiler::INITIALIZED)?;
    c.define_data_property_rooted(receiver, key, state, false, false, false)?;
    Ok(receiver)
}

fn now_milliseconds() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_millis() as f64)
}

/// `profiler.start()`: begins once.
pub(crate) fn gc_profiler_start(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = profiler_state(c, receiver)?;
    let current = get(c, state, profiler::STATE)?;
    if c.to_number(current)? == profiler::INITIALIZED {
        put_number(c, state, profiler::START_TIME, now_milliseconds())?;
        put_number(c, state, profiler::STATE, profiler::STARTED)?;
    }
    Ok(c.undefined())
}

/// `profiler.stop()`: the profile once started; Quench reports no collector events.
pub(crate) fn gc_profiler_stop(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = profiler_state(c, receiver)?;
    let current = get(c, state, profiler::STATE)?;
    if c.to_number(current)? != profiler::STARTED {
        return Ok(c.undefined());
    }
    put_number(c, state, profiler::STATE, profiler::STOPPED)?;
    let profile = c.object_rooted()?;
    put_number(c, profile, "version", profiler::VERSION)?;
    let start = get(c, state, profiler::START_TIME)?;
    put(c, profile, "startTime", start)?;
    let statistics = c.array_rooted(&[])?;
    put(c, profile, "statistics", statistics)?;
    put_number(c, profile, "endTime", now_milliseconds())?;
    Ok(profile)
}

/// `class Sub extends Base`: the constructor and prototype chains.
fn extend(c: &mut Context<'_>, class: RootId, base: RootId) -> Result<RootId, RootedError> {
    c.set_prototype_rooted(class, base)?;
    let prototype = get(c, class, "prototype")?;
    let base_prototype = get(c, base, "prototype")?;
    c.set_prototype_rooted(prototype, base_prototype)?;
    Ok(prototype)
}

fn serdes_class(c: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    let binding = super::serdes_shared_vm::binding(c)?;
    get(c, binding, name)
}

/// `new DefaultSerializer()`: a `Serializer` writing array buffer views as host objects.
pub(crate) fn default_serializer(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    super::serdes_shared_vm::serializer(c, receiver, &[])?;
    let yes = c.boolean(true);
    super::serdes_shared_vm::treat_views_as_host_objects(c, receiver, &[yes])?;
    Ok(receiver)
}

/// `new DefaultDeserializer(buffer)`.
pub(crate) fn default_deserializer(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    super::serdes_shared_vm::deserializer(c, receiver, args)
}

/// `DefaultSerializer.prototype._writeHostObject(view)`.
pub(crate) fn write_host_object(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let view = argument(c, args, 0);
    let output = super::serdes_shared_vm::serializer_output(c, receiver)?;
    if !super::serdes_shared_vm::write_view(c, &output, view)? {
        let detail = super::console_format_shared_vm::inspect(c, view, Default::default())?;
        let error_class = get(c, receiver, "_getDataCloneError")?;
        let message = c.string_rooted(&format!("Unserializable host object: {detail}"));
        let error = c.construct_rooted(error_class, error_class, &[message])?;
        return Err(c.throw(error));
    }
    Ok(c.undefined())
}

/// `DefaultDeserializer.prototype._readHostObject()`.
pub(crate) fn read_host_object(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let input = super::serdes_shared_vm::deserializer_input(c, receiver)?;
    let view = get(c, receiver, "buffer")?;
    let storage = c.buffer_storage_rooted(view)?;
    let Some(storage) = storage else {
        return Err(super::serdes_shared_vm::throw_error(c, super::serdes_shared_vm::DESERIALIZATION_ERROR));
    };
    match super::serdes_shared_vm::read_view(c, &input, storage.buffer, storage.byte_offset)? {
        Some(view) => Ok(view),
        None => {
            let error = c.type_error_rooted("ctor is not a constructor")?;
            Err(c.throw(error))
        }
    }
}

/// `Deserializer.prototype.readRawBytes(length)`: a Buffer over the next `length` bytes.
pub(crate) fn read_raw_bytes(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let length = argument(c, args, 0);
    let offset = super::serdes_shared_vm::read_raw_bytes(c, receiver, &[length])?;
    let view = get(c, receiver, "buffer")?;
    let storage = c.buffer_storage_rooted(view)?.ok_or_else(|| RootedError::host("deserializer buffer is no view"))?;
    let offset = c.to_number(offset)? + storage.byte_offset as f64;
    let buffer = super::buffer_shared_vm::fast_buffer_constructor(c)?;
    let offset = c.number(offset);
    c.construct_rooted(buffer, buffer, &[storage.buffer, offset, length])
}

fn classes(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0builtin:v8-classes", |c| {
        let classes = c.null_object_rooted()?;
        let serializer = serdes_class(c, "Serializer")?;
        let prototype = get(c, serializer, "prototype")?;
        let global = c.global_root()?;
        let error = get(c, global, "Error")?;
        put(c, prototype, "_getDataCloneError", error)?;
        let deserializer = serdes_class(c, "Deserializer")?;
        let prototype = get(c, deserializer, "prototype")?;
        let read = c.host_function_named(crate::host::shared_vm::operation!("v8ReadRawBytes"), "readRawBytes")?;
        put(c, prototype, "readRawBytes", read)?;
        for (name, operation, base, method, method_operation) in [
            ("DefaultSerializer", "v8DefaultSerializer", serializer, "_writeHostObject", "v8WriteHostObject"),
            ("DefaultDeserializer", "v8DefaultDeserializer", deserializer, "_readHostObject", "v8ReadHostObject"),
        ] {
            let class = c.host_constructor_named(crate::host::shared_vm::operation(operation), name)?;
            let prototype = extend(c, class, base)?;
            let function = c.host_function_named(crate::host::shared_vm::operation(method_operation), method)?;
            let key = c.string_rooted(method);
            c.define_data_property_rooted(prototype, key, function, true, false, true)?;
            put(c, classes, name, class)?;
        }
        put(c, classes, "Serializer", serializer)?;
        put(c, classes, "Deserializer", deserializer)?;
        Ok(classes)
    })
}

/// `serialize(value)`: the value through a `DefaultSerializer`, header first.
pub(crate) fn serialize(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = argument(c, args, 0);
    let classes = classes(c)?;
    let class = get(c, classes, "DefaultSerializer")?;
    let serializer = c.construct_rooted(class, class, &[])?;
    super::stream_shared_vm::call_method(c, serializer, "writeHeader", &[])?;
    super::stream_shared_vm::call_method(c, serializer, "writeValue", &[value])?;
    super::stream_shared_vm::call_method(c, serializer, "releaseBuffer", &[])
}

/// `deserialize(buffer)`: the value a `DefaultDeserializer` reads after the header.
pub(crate) fn deserialize(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let buffer = argument(c, args, 0);
    let classes = classes(c)?;
    let class = get(c, classes, "DefaultDeserializer")?;
    let deserializer = c.construct_rooted(class, class, &[buffer])?;
    super::stream_shared_vm::call_method(c, deserializer, "readHeader", &[])?;
    super::stream_shared_vm::call_method(c, deserializer, "readValue", &[])
}

/// `node:v8`'s exports, in Node's order.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = c.object_rooted()?;
    let classes = classes(c)?;
    let function = |c: &mut Context<'_>, module: RootId, name: &str, operation: &str| -> Result<(), RootedError> {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        put(c, module, name, function)
    };
    for (name, operation) in [
        ("cachedDataVersionTag", "v8CachedDataVersionTag"),
        ("getHeapSnapshot", "v8GetHeapSnapshot"),
        ("getHeapStatistics", "v8GetHeapStatistics"),
        ("getHeapSpaceStatistics", "v8GetHeapSpaceStatistics"),
        ("getHeapCodeStatistics", "v8GetHeapCodeStatistics"),
        ("getCppHeapStatistics", "v8GetCppHeapStatistics"),
        ("setFlagsFromString", "v8SetFlagsFromString"),
    ] {
        function(c, module, name, operation)?;
    }
    for name in ["Serializer", "Deserializer", "DefaultSerializer", "DefaultDeserializer"] {
        let class = get(c, classes, name)?;
        put(c, module, name, class)?;
    }
    for (name, operation) in [
        ("deserialize", "v8Deserialize"),
        ("takeCoverage", "v8Coverage"),
        ("stopCoverage", "v8Coverage"),
        ("serialize", "v8Serialize"),
        ("writeHeapSnapshot", "v8WriteHeapSnapshot"),
    ] {
        function(c, module, name, operation)?;
    }
    let hooks = promise_hooks(c)?;
    put(c, module, "promiseHooks", hooks)?;
    function(c, module, "queryObjects", "v8QueryObjectsPublic")?;
    let snapshot = object_with_methods(
        c,
        &[
            ("addDeserializeCallback", "v8NotBuildingSnapshot"),
            ("addSerializeCallback", "v8NotBuildingSnapshot"),
            ("setDeserializeMainFunction", "v8NotBuildingSnapshot"),
            ("isBuildingSnapshot", "v8IsBuildingSnapshot"),
        ],
    )?;
    put(c, module, "startupSnapshot", snapshot)?;
    function(c, module, "setHeapSnapshotNearHeapLimit", "v8SetHeapSnapshotNearHeapLimit")?;
    let profiler = c.host_constructor_named(crate::host::shared_vm::operation!("v8GcProfiler"), "GCProfiler")?;
    let prototype = get(c, profiler, "prototype")?;
    for (name, operation) in [("start", "v8GcProfilerStart"), ("stop", "v8GcProfilerStop")] {
        let method = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, method, true, false, true)?;
    }
    put(c, module, "GCProfiler", profiler)?;
    function(c, module, "isStringOneByteRepresentation", "v8IsStringOneByteRepresentation")?;
    Ok(module)
}

/// `internalBinding('v8')`: the members Node's library reads that Quench keeps.
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:v8", |c| {
        object_with_methods(
            c,
            &[
                ("cachedDataVersionTag", "v8CachedDataVersionTag"),
                ("setFlagsFromString", "v8SetFlagsFromString"),
                ("setHeapSnapshotNearHeapLimit", "v8SetHeapSnapshotNearHeapLimit"),
                ("isStringOneByteRepresentation", "v8IsStringOneByteRepresentation"),
            ],
        )
    })
}

/// `internalBinding('heap_utils')`.
pub(crate) fn heap_utils_binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:heap_utils", |c| {
        object_with_methods(c, &[("triggerHeapSnapshot", "v8WriteHeapSnapshot"), ("createHeapSnapshotStream", "v8GetHeapSnapshot")])
    })
}

/// `internalBinding('internal_only_v8')`.
pub(crate) fn internal_only_binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:internal_only_v8", |c| {
        object_with_methods(c, &[("queryObjects", "v8QueryObjects")])
    })
}

/// `getHeapStatistics()`'s values, in `HEAP_STATISTICS` order.
fn heap_statistics(c: &mut Context<'_>) -> [f64; HEAP_STATISTICS.len()] {
    const NATIVE_CONTEXTS: f64 = 1.0;
    let [used, reserved, limit] = heap_sizes(c);
    [reserved, 0.0, reserved, (limit - reserved).max(0.0), used, limit, 0.0, 0.0, 0.0, NATIVE_CONTEXTS, 0.0, 0.0, 0.0, 0.0]
}

/// One space's statistics, in `HEAP_SPACE_STATISTICS` order: Quench's heap is `HEAP_SPACE`.
fn heap_space_statistics(c: &mut Context<'_>, name: &str) -> [f64; HEAP_SPACE_STATISTICS.len()] {
    if name != HEAP_SPACE {
        return [0.0; HEAP_SPACE_STATISTICS.len()];
    }
    let [used, reserved, limit] = heap_sizes(c);
    [reserved, used, (limit - reserved).max(0.0), reserved]
}

/// `ScriptCompiler::CachedDataVersionTag()`: Quench's code cache format is its own, so the
/// tag names this engine build rather than V8's.
pub(crate) fn cached_data_tag() -> u32 {
    const FNV_OFFSET: u32 = 0x811c_9dc5;
    const FNV_PRIME: u32 = 0x0100_0193;
    let identity = concat!("quench-", env!("CARGO_PKG_VERSION"));
    identity.bytes().fold(FNV_OFFSET, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(FNV_PRIME))
}

pub(crate) fn cached_data_version_tag(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.number(f64::from(cached_data_tag())))
}

pub(crate) fn is_string_one_byte_representation(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = match args.first() {
        Some(value) => c.string_code_units(*value)?,
        None => None,
    };
    let one_byte = value.is_some_and(|units| units.iter().all(|unit| *unit <= u16::from(u8::MAX)));
    Ok(c.boolean(one_byte))
}

/// `internalBinding('internal_only_v8').queryObjects(prototype)`: the heap's objects whose
/// prototype is `prototype`.
pub(crate) fn query_objects(
    context: &mut NativeContext<'_, NodeHost>,
    _receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let prototype = args.first().copied().unwrap_or_else(|| context.undefined());
    let objects = context.objects_with_prototype_rooted(prototype)?;
    context.array_rooted(&objects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_old_space_size_sets_the_old_generation() {
        let limit = heap_size_limit(&["--max-old-space-size=512".to_owned()]);
        assert_eq!(limit, 512 * MIB + YOUNG_GENERATION_SEMI_SPACES * MAX_SEMI_SPACE_SIZE);
    }
}
