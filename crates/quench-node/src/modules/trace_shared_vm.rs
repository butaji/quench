//! Native trace category policy. The syntax bridge supplies ordinary methods
//! and private WeakMap storage; enabled objects are retained by the process.
use super::event_constructor_shared_vm::get;
use super::trace_state::EnabledTracing;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const MAX_ENABLED_TRACING_WITHOUT_WARNING: usize = 10;
const MAX_SAFE_HANDLE: u64 = (1u64 << 53) - 1;
const FACTORY: &str = r#"(api => {
  'use strict';
  class Tracing {
    constructor(categories) { api.initialize(this, categories); }
    enable() { return api.enable(this); }
    disable() { return api.disable(this); }
    get enabled() { return api.enabled(this); }
    get categories() { return api.categories(this); }
    [api.inspectSymbol](depth, opts) { return api.inspect(this, depth, opts); }
  }
  api.constructor = Tracing;
  return {
    createTracing: function createTracing(options) { return api.create(options); },
    getEnabledCategories: function getEnabledCategories() { return api.getEnabledCategories(); }
  };
})"#;

fn put(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.define_data_property_rooted(object, key, value, true, true, true)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot define native state {name}"
        )))
    }
}
fn call(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let method = get(context, object, name)?;
    context.call_rooted(method, object, args)
}
fn undefined(context: &NativeContext<'_, NodeHost>, root: RootId) -> bool {
    context.rooted_value(root).is_some_and(|v| v.is_undefined())
}
fn brand(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    member: &str,
) -> Result<RootId, RootedError> {
    let api = context.host_function_data()?;
    let map = get(context, api, "states")?;
    let state = context.weak_map_get_rooted(map, object)?;
    if undefined(context, state) {
        let error = context.type_error_rooted(&format!(
            "Cannot read private member #{member} from an object whose class did not declare it"
        ))?;
        return Err(context.throw(error));
    }
    Ok(state)
}
fn handle(context: &mut NativeContext<'_, NodeHost>, state: RootId) -> Result<u64, RootedError> {
    let value = get(context, state, "handle")?;
    context
        .rooted_value(value)
        .and_then(|v| v.as_number())
        .map(|v| v as u64)
        .ok_or_else(|| RootedError::host("invalid native tracing handle"))
}
fn string_array(
    context: &mut NativeContext<'_, NodeHost>,
    array: RootId,
    validate: bool,
) -> Result<Vec<String>, RootedError> {
    if !context.is_array_rooted(array)? {
        return Err(super::util_shared_vm::argument_type_error(
            context,
            array,
            "options.categories",
            "Array",
        )?);
    }
    if validate {
        get(context, array, "length")?;
    }
    let mut strings = Vec::new();
    let mut index = 0usize;
    loop {
        let length = get(context, array, "length")?;
        let length = context
            .rooted_value(length)
            .and_then(|v| v.as_number())
            .unwrap_or(0.0) as usize;
        if index >= length {
            break;
        }
        let key = context.string_rooted(&index.to_string());
        let value = context.get_property_rooted(array, key)?;
        if validate && context.string_text(value)?.is_none() {
            let received = context.get_property_rooted(array, key)?;
            return Err(super::util_shared_vm::argument_type_error(
                context,
                received,
                &format!("options.categories[{index}]"),
                "string",
            )?);
        }
        if !validate {
            strings.push(context.to_string(value)?);
        }
        index += 1;
    }
    Ok(strings)
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    // Node disables trace_events inside Worker isolates because the tracing
    // provider is owned by the process main thread. Keep the module unavailable
    // in the same way instead of exposing an isolated per-worker trace facade.
    if crate::modules::worker_threads_shared_vm::is_child(context) {
        let error = context.error_rooted("Trace events are unavailable")?;
        let code = context.string_rooted("ERR_TRACE_EVENTS_UNAVAILABLE");
        put(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    let api = context.null_object_rooted()?;
    let state =
        context.evaluate_script_rooted("new WeakMap()", "node:trace_events/private-state")?;
    let global = context.global_root()?;
    let weak_map = get(context, global, "WeakMap")?;
    let weak_prototype = get(context, weak_map, "prototype")?;
    let map_set = get(context, weak_prototype, "set")?;
    let array = get(context, global, "Array")?;
    let array_prototype = get(context, array, "prototype")?;
    let join = get(context, array_prototype, "join")?;
    put(context, api, "states", state)?;
    let symbol = get(context, global, "Symbol")?;
    let symbol_for = get(context, symbol, "for")?;
    let inspect_name = context.string_rooted("nodejs.util.inspect.custom");
    let inspect_symbol = context.call_rooted(symbol_for, symbol, &[inspect_name])?;
    put(context, api, "inspectSymbol", inspect_symbol)?;
    put(context, api, "mapSet", map_set)?;
    put(context, api, "join", join)?;
    for (name, operation) in [
        ("initialize", "traceInitialize"),
        ("inspect", "traceInspect"),
        ("create", "traceCreate"),
        ("enable", "traceEnable"),
        ("disable", "traceDisable"),
        ("categories", "traceCategories"),
        ("enabled", "traceEnabled"),
        ("getEnabledCategories", "traceGetEnabledCategories"),
    ] {
        let function =
            context.host_function_with_data(crate::host::shared_vm::operation(operation), api)?;
        put(context, api, name, function)?;
    }
    let factory = context.evaluate_script_rooted(FACTORY, "node:trace_events/methods")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[api])
}

pub(crate) fn create(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let options = args.first().copied().unwrap_or(absent);
    if !context.is_object_rooted(options)? || context.is_array_rooted(options)? {
        return Err(super::util_shared_vm::argument_type_error(
            context, options, "options", "Object",
        )?);
    }
    let categories = get(context, options, "categories")?;
    string_array(context, categories, true)?;
    let categories = get(context, options, "categories")?;
    let length = get(context, categories, "length")?;
    let zero = context.number(0.0);
    if !context.greater_than_rooted(length, zero)? {
        let error = context.type_error_rooted("At least one category is required")?;
        let code = context.string_rooted("ERR_TRACE_EVENTS_CATEGORY_REQUIRED");
        put(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    let original = get(context, options, "categories")?;
    let api = context.host_function_data()?;
    let constructor = get(context, api, "constructor")?;
    context.construct_rooted(constructor, constructor, &[original])
}

pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let object = args.first().copied().unwrap_or(absent);
    let original = args.get(1).copied().unwrap_or(absent);
    let snapshot = string_array(context, original, false)?;
    let roots: Vec<_> = snapshot.iter().map(|s| context.string_rooted(s)).collect();
    let snapshot = context.array_rooted(&roots)?;
    let shared = context.host_mut().shared_state();
    let id = {
        let mut shared = shared.borrow_mut();
        let id = shared.trace.next_handle;
        if id > MAX_SAFE_HANDLE {
            return Err(RootedError::host("tracing handle space exhausted"));
        }
        shared.trace.next_handle += 1;
        id
    };
    let api = context.host_function_data()?;
    let state = context.null_object_rooted()?;
    let id = context.number(id as f64);
    put(context, state, "handle", id)?;
    put(context, state, "original", original)?;
    put(context, state, "snapshot", snapshot)?;
    let map = get(context, api, "states")?;
    let set = get(context, api, "mapSet")?;
    context.call_rooted(set, map, &[object, state])?;
    Ok(absent)
}

pub(crate) fn enable(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let object = args.first().copied().unwrap_or(absent);
    let state = brand(context, object, "enabled")?;
    let id = handle(context, state)?;
    let shared = context.host_mut().shared_state();
    if shared.borrow().trace.enabled.contains_key(&id) {
        return Ok(absent);
    }
    let snapshot = get(context, state, "snapshot")?;
    let categories = string_array(context, snapshot, false)?;
    let retained = context.retain(object)?;
    if !categories.is_empty() {
        let directory = shared.borrow().cwd.path();
        shared.borrow_mut().trace.activate(&directory);
    }
    shared.borrow_mut().trace.enabled.insert(
        id,
        EnabledTracing {
            retained,
            categories,
        },
    );
    refresh_category_buffers(context)?;
    if shared.borrow().trace.enabled.len() > MAX_ENABLED_TRACING_WITHOUT_WARNING {
        let global = context.global_root()?;
        let process = get(context, global, "process")?;
        let message = context.string_rooted("Possible trace_events memory leak detected. There are more than 10 enabled Tracing objects.");
        call(context, process, "emitWarning", &[message])?;
    }
    Ok(absent)
}
pub(crate) fn disable(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let object = args.first().copied().unwrap_or(absent);
    let state = brand(context, object, "enabled")?;
    let id = handle(context, state)?;
    let shared = context.host_mut().shared_state();
    let removed = shared.borrow_mut().trace.enabled.remove(&id);
    if let Some(entry) = removed {
        context.release_root(entry.retained);
    }
    refresh_category_buffers(context)?;
    Ok(absent)
}
pub(crate) fn enabled(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let state = brand(context, args.first().copied().unwrap_or(absent), "enabled")?;
    let id = handle(context, state)?;
    let enabled = context
        .host_mut()
        .shared_state()
        .borrow()
        .trace
        .enabled
        .contains_key(&id);
    Ok(context.boolean(enabled))
}
pub(crate) fn categories(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let state = brand(
        context,
        args.first().copied().unwrap_or(absent),
        "categories",
    )?;
    let original = get(context, state, "original")?;
    let api = context.host_function_data()?;
    let join = get(context, api, "join")?;
    let separator = context.string_rooted(",");
    context.call_rooted(join, original, &[separator])
}
pub(crate) fn get_enabled_categories(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let categories = context
        .host_mut()
        .shared_state()
        .borrow()
        .trace
        .enabled_categories();
    Ok(match categories {
        Some(text) => context.string_rooted(&text),
        None => context.undefined(),
    })
}

pub(crate) fn inspect(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let object = args.first().copied().unwrap_or(absent);
    let depth = args.get(1).copied().unwrap_or(absent);
    if context
        .rooted_value(depth)
        .and_then(|v| v.as_number())
        .is_some_and(|v| v < 0.0)
    {
        return Ok(object);
    }
    let enabled = get(context, object, "enabled")?;
    let categories = get(context, object, "categories")?;
    let projection = context.object_rooted()?;
    put(context, projection, "enabled", enabled)?;
    put(context, projection, "categories", categories)?;
    let inspect = super::util_shared_vm::internal_inspect(context)?;
    let text = context.call_rooted(inspect, absent, &[projection])?;
    let text = context.to_string(text)?;
    Ok(context.string_rooted(&format!("Tracing {text}")))
}

pub(crate) fn internal_binding(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let name = context.to_string(args.first().copied().unwrap_or(absent))?;
    let key = format!("\0binding:{name}");
    let shared = context.host_mut().shared_state();
    if let Some(value) = shared.borrow().builtin_cache.get(&key).copied() {
        return Ok(value);
    }
    let binding = build_internal_binding(context, &name)?;
    if !shared.borrow().builtin_cache.contains_key(&key) {
        let retained = context.retain(binding)?;
        shared.borrow_mut().builtin_cache.insert(key, retained);
    }
    Ok(binding)
}

fn build_internal_binding(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
) -> Result<RootId, RootedError> {
    match name {
        #[cfg(target_os = "linux")]
        "os" => return super::os_binding_shared_vm::os_binding(context),
        #[cfg(target_os = "linux")]
        "credentials" => return super::os_binding_shared_vm::credentials_binding(context),
        "fs" => return super::fs_directory_shared_vm::binding(context),
        "errors" => return super::node_errors_shared_vm::binding(context),
        "report" => return super::report_configuration_shared_vm::binding(context),
        #[cfg(target_os = "linux")]
        "process_methods" => return super::process_metrics_shared_vm::binding(context),
        "modules" => {
            return crate::host::shared_vm::commonjs::cached_native_binding(
                context,
                "\0binding:modules",
                module_protocol_binding,
            );
        }
        "stream_wrap" => return super::stream_binding::binding(context),
        "tty_wrap" => return tty_binding(context),
        "udp_wrap" => return super::udp_binding::binding(context),
        "pipe_wrap" => return super::pipe_binding::binding(context),
        "tcp_wrap" => return super::tcp_binding::binding(context),
        "contextify" => return super::contextify_shared_vm::binding(context),
        "types" => return super::util_shared_vm::types_binding(context),
        "messaging" => return super::messaging_binding_shared_vm::binding(context),
        "crypto_keys" => return super::crypto_key_shared_vm::binding(context),
        "util" => return super::util_shared_vm::key_binding(context),
        "options" => return super::cli_options_shared_vm::binding(context),
        "string_decoder" => return super::string_decoder_shared_vm::binding(context),
        "zlib" => return super::zlib_native_shared_vm::binding(context),
        "encoding_binding" => return super::encoding_binding_shared_vm::binding(context),
        #[cfg(quench_icu)]
        "icu" => return super::icu_binding_shared_vm::binding(context),
        "config" => {
            let binding = context.object_rooted()?;
            let has_intl = context.boolean(cfg!(quench_icu));
            put(context, binding, "hasIntl", has_intl)?;
            Ok(binding)
        }
        "symbols" => return super::util_shared_vm::symbols_binding(context),
        "cares_wrap" => return crate::host::shared_vm::commonjs::cares_binding(context),
        "http_parser" => return super::http_common_shared_vm::module(context),
        "http2" => return crate::host::shared_vm::commonjs::http2_binding(context),
        "buffer" => return super::buffer_fill_shared_vm::binding(context),
        "crypto" => return super::tls_shared_vm::facade::binding(context),
        "timers" => return super::timers_shared_vm::binding(context),
        "performance" => return super::performance_binding_shared_vm::binding(context),
        "uv" => return super::util_system_error_shared_vm::uv_binding(context),
        "fs_event_wrap" => return super::fs_watch_shared_vm::event_binding(context),
        "trace_events" => {
            let binding = context.object_rooted()?;
            for (name, operation) in [
                ("getCategoryEnabledBuffer", "traceCategoryBuffer"),
                ("trace", "traceRecord"),
                ("isTraceCategoryEnabled", "traceCategoryEnabled"),
                ("getEnabledCategories", "traceGetEnabledCategories"),
            ] {
                let function =
                    context.host_function(crate::host::shared_vm::operation(operation))?;
                put(context, binding, name, function)?;
            }
            let perfetto = context.boolean(false);
            put(context, binding, "usePerfetto", perfetto)?;
            Ok(binding)
        }
        "cjs_lexer" => {
            let binding = context.object_rooted()?;
            let parse =
                context.host_function(crate::host::shared_vm::operation("cjsLexerParse"))?;
            put(context, binding, "parse", parse)?;
            Ok(binding)
        }
        "constants" => {
            let binding = context.object_rooted()?;
            let fs = super::fs_shared_vm::fs_constants(context)?;
            put(context, binding, "fs", fs)?;
            #[cfg(target_os = "linux")]
            {
                let os = super::os_binding_shared_vm::constants(context)?;
                put(context, binding, "os", os)?;
            }
            let zlib = super::zlib_shared_vm::constants(context)?;
            put(context, binding, "zlib", zlib)?;
            let crypto = super::crypto_constants::binding(context)?;
            put(context, binding, "crypto", crypto)?;
            let trace = context.object_rooted()?;
            for (name, code) in [
                ("TRACE_EVENT_PHASE_BEGIN", b'B'),
                ("TRACE_EVENT_PHASE_END", b'E'),
                ("TRACE_EVENT_PHASE_INSTANT", b'I'),
                ("TRACE_EVENT_PHASE_NESTABLE_ASYNC_BEGIN", b'b'),
                ("TRACE_EVENT_PHASE_NESTABLE_ASYNC_END", b'e'),
                ("TRACE_EVENT_PHASE_NESTABLE_ASYNC_INSTANT", b'n'),
                ("TRACE_EVENT_PHASE_COUNTER", b'C'),
            ] {
                let code = context.number(f64::from(code));
                put(context, trace, name, code)?;
            }
            put(context, binding, "trace", trace)?;
            Ok(binding)
        }
        _ => {
            let error = context.error_rooted(&format!("No such binding: {name}"))?;
            let code = context.string_rooted("ERR_INVALID_MODULE");
            put(context, error, "code", code)?;
            return Err(context.throw(error));
        }
    }
}

pub(crate) fn cjs_lexer_parse(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let source = args.first().copied().unwrap_or(absent);
    let source_text = if undefined(context, source)
        || context
            .rooted_value(source)
            .is_some_and(|value| value.is_null())
    {
        String::new()
    } else if let Some(source) = context.string_text(source)? {
        source
    } else {
        let error = context.type_error_rooted("source must be a string")?;
        return Err(context.throw(error));
    };
    let (exports, reexports) = crate::host::shared_vm::commonjs_exports::parse(&source_text);
    let export_values = exports
        .iter()
        .map(|name| context.string_rooted(name))
        .collect::<Vec<_>>();
    let reexport_values = reexports
        .iter()
        .map(|name| context.string_rooted(name))
        .collect::<Vec<_>>();
    let export_values = context.array_rooted(&export_values)?;
    let reexport_values = context.array_rooted(&reexport_values)?;
    let global = context.global_root()?;
    let set_constructor = get(context, global, "Set")?;
    let export_set =
        context.construct_rooted(set_constructor, set_constructor, &[export_values])?;
    context.array_rooted(&[export_set, reexport_values])
}

/// Surface the parts of Node's tty_wrap binding used by the public tty module
/// and by code that inspects native stream prototype descriptors. The runtime
/// does not expose a terminal handle, so isTTY reports false and the handle
/// constructor preserves the public shape while its descriptor accessors
/// reject forged receivers like Node's native accessors do.
fn tty_binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let source = r#"(() => {
      const streamBase = {};
      const invalid = function() { throw new TypeError('Illegal invocation'); };
      for (const name of ['bytesRead', 'fd', '_externalStream']) {
        Object.defineProperty(streamBase, name, { get: invalid, configurable: true });
      }
      function TTY(fd, ctx) {
        if (!(this instanceof TTY)) return new TTY(fd, ctx);
        this._handle = this;
        this.fd = fd;
        if (ctx && typeof ctx === 'object') ctx.code = undefined;
      }
      TTY.prototype = Object.create(streamBase);
      Object.defineProperties(TTY.prototype, {
        constructor: { value: TTY, writable: true, configurable: true },
        setBlocking: { value() {}, writable: true, configurable: true },
        setRawMode: { value() { return 0; }, writable: true, configurable: true },
        getWindowSize: { value(size) { if (size) { size[0] = 0; size[1] = 0; } return 0; }, writable: true, configurable: true },
        readStart: { value() { return 0; }, writable: true, configurable: true },
        readStop: { value() { return 0; }, writable: true, configurable: true },
        close: { value() {}, writable: true, configurable: true }
      });
      return {
        TTY,
        isTTY() { return false; },
        UV_TTY_MODE_NORMAL: 0,
        UV_TTY_MODE_RAW: 1,
        UV_TTY_MODE_IO: 2,
        UV_TTY_MODE_RAW_VT: 3
      };
    })()"#;
    context.evaluate_script_rooted(source, "internalBinding/tty_wrap.js")
}

fn required_string(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    description: &str,
) -> Result<String, RootedError> {
    if let Some(text) = context.string_text(value)? {
        return Ok(text);
    }
    let error =
        context.type_error_rooted(&format!("Trace event {description} must be a string."))?;
    Err(context.throw(error))
}
fn int32(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    description: &str,
) -> Result<i32, RootedError> {
    if context
        .rooted_value(value)
        .and_then(|v| v.as_number())
        .is_none()
    {
        let error =
            context.type_error_rooted(&format!("Trace event {description} must be a number."))?;
        return Err(context.throw(error));
    }
    let zero = context.number(0.0);
    let converted = context.bitwise_or_rooted(value, zero)?;
    Ok(context
        .rooted_value(converted)
        .and_then(|v| v.as_number())
        .unwrap_or(0.0) as i32)
}
pub(crate) fn category_enabled(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let category = required_string(context, args.first().copied().unwrap_or(absent), "category")?;
    let enabled = context
        .host_mut()
        .shared_state()
        .borrow()
        .trace
        .category_enabled(&category);
    Ok(context.boolean(enabled))
}
pub(crate) fn record(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use super::trace_state::Phase;
    let absent = context.undefined();
    let category = required_string(context, args.get(1).copied().unwrap_or(absent), "category")?;
    if !context
        .host_mut()
        .shared_state()
        .borrow()
        .trace
        .category_enabled(&category)
    {
        return Ok(context.boolean(false));
    }
    let phase = int32(context, args.first().copied().unwrap_or(absent), "phase")? as u8;
    let name = required_string(context, args.get(2).copied().unwrap_or(absent), "name")?;
    let raw_id = args.get(3).copied().unwrap_or(absent);
    let id = if context
        .rooted_value(raw_id)
        .is_some_and(|v| v.is_null() || v.is_undefined())
    {
        None
    } else {
        Some(int32(context, raw_id, "id")? as i64 as u64)
    };
    if name.is_empty() {
        let error = context.type_error_rooted("Trace event name must not be an empty string.")?;
        return Err(context.throw(error));
    }
    let mut data = serde_json::json!({});
    let raw_data = args.get(4).copied().unwrap_or(absent);
    if !undefined(context, raw_data) {
        let serialized = context.stringify_json_rooted(raw_data)?;
        if !undefined(context, serialized) {
            let serialized = context.to_string(serialized)?;
            data["data"] =
                serde_json::from_str(&serialized).map_err(|e| RootedError::host(e.to_string()))?;
        }
    }
    context.host_mut().shared_state().borrow_mut().trace.record(
        &category,
        &name,
        Phase::from_code(phase),
        id,
        data,
    );
    Ok(context.boolean(true))
}

pub(crate) fn category_buffer(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let category = required_string(context, args.first().copied().unwrap_or(absent), "category")?;
    let enabled = context
        .host_mut()
        .shared_state()
        .borrow()
        .trace
        .category_enabled(&category);
    let buffer = context.uint8_array_rooted(&[u8::from(enabled)])?;
    let weak = context.host_weak_root(buffer)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .trace
        .category_buffers
        .push((category, weak));
    Ok(buffer)
}
fn refresh_category_buffers(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let shared = context.host_mut().shared_state();
    let mut buffers = std::mem::take(&mut shared.borrow_mut().trace.category_buffers);
    let key = context.string_rooted("0");
    let mut result = Ok(());
    buffers.retain(|(category, weak)| {
        // Preserve remaining registrations if a core write fails. The Rust
        // borrow never crosses VM entry, and the registry is restored below.
        if result.is_err() {
            return true;
        }
        if let Some(buffer) = context.resolve_host_weak_root(*weak) {
            let enabled = shared.borrow().trace.category_enabled(category);
            let value = context.number(f64::from(u8::from(enabled)));
            result = context
                .set_property_rooted(buffer, key, value, buffer)
                .map(|_| ());
            true
        } else {
            context.release_host_weak_root(*weak);
            false
        }
    });
    shared.borrow_mut().trace.category_buffers = buffers;
    result
}

fn module_protocol_binding(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    let statuses = super::node_protocol_constants::COMPILE_CACHE_STATUS
        .iter()
        .map(|status| context.string_rooted(status))
        .collect::<Vec<_>>();
    let statuses = context.array_rooted(&statuses)?;
    put(context, binding, "compileCacheStatus", statuses)?;
    let types = context.null_object_rooted()?;
    for &(name, value) in super::node_protocol_constants::CACHED_CODE_TYPES {
        let value = context.number(f64::from(value));
        put(context, types, name, value)?;
    }
    put(context, binding, "cachedCodeTypes", types)?;
    Ok(binding)
}
