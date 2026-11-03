//! `trace_events` (`lib/trace_events.js`) and the internal bindings the host serves.
//!
//! A `Tracing` keeps its handle, categories and category snapshot in a private slot; the
//! process retains each enabled one (`TraceState::enabled`).
use super::event_constructor_shared_vm::get;
use super::trace_state::EnabledTracing;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const MAX_ENABLED_TRACING_WITHOUT_WARNING: usize = 10;
const MAX_SAFE_HANDLE: u64 = (1u64 << 53) - 1;

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
    let slot = get(context, api, "slot")?;
    let state = if context.is_object_rooted(object)? {
        context.get_property_rooted(object, slot)?
    } else {
        context.undefined()
    };
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
    if context.host_mut().shared_state().borrow().messaging.is_worker() {
        let error = context.error_rooted("Trace events are unavailable")?;
        let code = context.string_rooted("ERR_TRACE_EVENTS_UNAVAILABLE");
        put(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    let api = context.null_object_rooted()?;
    let slot = context.private_symbol_rooted(Some("Tracing"));
    put(context, api, "slot", slot)?;
    let global = context.global_root()?;
    let array = get(context, global, "Array")?;
    let array_prototype = get(context, array, "prototype")?;
    let join = context.startup_data_property_rooted(array_prototype, "join").map_or_else(|| get(context, array_prototype, "join"), Ok)?;
    put(context, api, "join", join)?;
    let class = context.host_constructor_named_with_data(crate::host::shared_vm::operation!("traceInitialize"), "Tracing", api)?;
    let prototype = get(context, class, "prototype")?;
    for (name, operation) in TRACING_METHODS {
        let function = context.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, api)?;
        context.define_named_data_property_rooted(prototype, name, function, true, false, true)?;
    }
    for (name, operation) in TRACING_GETTERS {
        let getter = context.host_function_named_with_data(crate::host::shared_vm::operation(operation), &format!("get {name}"), api)?;
        let key = context.string_rooted(name);
        let undefined = context.undefined();
        let fields = quench_runtime::DescriptorFields {
            get: Some(getter),
            set: Some(undefined),
            enumerable: Some(false),
            configurable: Some(true),
            ..Default::default()
        };
        context.define_property_fields_rooted(prototype, key, fields)?;
    }
    let symbol = get(context, global, "Symbol")?;
    let symbol_for = get(context, symbol, "for")?;
    let inspect_name = context.string_rooted("nodejs.util.inspect.custom");
    let inspect_symbol = context.call_rooted(symbol_for, symbol, &[inspect_name])?;
    let inspect = context.host_function_named_with_data(crate::host::shared_vm::operation!("traceInspect"), "[nodejs.util.inspect.custom]", api)?;
    context.define_data_property_rooted(prototype, inspect_symbol, inspect, true, false, true)?;
    put(context, api, "constructor", class)?;
    let module = context.object_rooted()?;
    for (name, operation) in MODULE_FUNCTIONS {
        let function = context.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, api)?;
        put(context, module, name, function)?;
    }
    Ok(module)
}

/// `Tracing.prototype`'s methods and getters, and the module's functions.
const TRACING_METHODS: [(&str, &str); 2] = [("enable", "traceEnable"), ("disable", "traceDisable")];
const TRACING_GETTERS: [(&str, &str); 2] = [("enabled", "traceEnabled"), ("categories", "traceCategories")];
const MODULE_FUNCTIONS: [(&str, &str); 2] = [("createTracing", "traceCreate"), ("getEnabledCategories", "traceGetEnabledCategories")];

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

/// `new Tracing(categories)`: its handle, `categories` and their snapshot in its slot.
pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let original = args.first().copied().unwrap_or(absent);
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
    let slot = get(context, api, "slot")?;
    context.define_data_property_rooted(object, slot, state, false, false, false)?;
    Ok(absent)
}

pub(crate) fn enable(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
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
    object: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
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
    object: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let state = brand(context, object, "enabled")?;
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
    object: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let state = brand(context, object, "categories")?;
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
    object: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let depth = args.first().copied().unwrap_or(absent);
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
        #[cfg(unix)]
        "os" => super::os_binding_shared_vm::os_binding(context),
        // Node's `lib/os.js` reads `getTempDir` from this binding at load time.
        #[cfg(unix)]
        "credentials" => super::os_binding_shared_vm::credentials_binding(context),
        "fs" => super::fs_directory_shared_vm::binding(context),
        "errors" => super::node_errors_shared_vm::binding(context),
        "report" => super::report_configuration_shared_vm::binding(context),
        #[cfg(unix)]
        "process_methods" => super::process_metrics_shared_vm::binding(context),
        "modules" => {
            crate::host::shared_vm::commonjs::cached_native_binding(
                context,
                "\0binding:modules",
                module_protocol_binding,
            )
        }
        "stream_wrap" => super::stream_binding::binding(context),
        "tty_wrap" => tty_binding(context),
        "udp_wrap" => super::udp_binding::binding(context),
        "pipe_wrap" => super::pipe_binding::binding(context),
        "tcp_wrap" => super::tcp_binding::binding(context),
        "contextify" => super::contextify_shared_vm::binding(context),
        "types" => super::util_shared_vm::types_binding(context),
        "messaging" => super::messaging_binding_shared_vm::binding(context),
        "crypto_keys" => super::crypto_key_shared_vm::binding(context),
        "util" => super::util_shared_vm::key_binding(context),
        "options" => super::cli_options_shared_vm::binding(context),
        "string_decoder" => super::string_decoder_shared_vm::binding(context),
        "zlib" => super::zlib_native_shared_vm::binding(context),
        "serdes" => super::serdes_shared_vm::binding(context),
        "ipc_serdes" => super::ipc_serdes_shared_vm::binding(context),
        "v8" => super::v8_shared_vm::binding(context),
        "heap_utils" => super::v8_shared_vm::heap_utils_binding(context),
        "internal_only_v8" => super::v8_shared_vm::internal_only_binding(context),
        "encoding_binding" => super::encoding_binding_shared_vm::binding(context),
        #[cfg(quench_icu)]
        "icu" => super::icu_binding_shared_vm::binding(context),
        "config" => {
            let binding = context.object_rooted()?;
            let has_intl = context.boolean(cfg!(quench_icu));
            put(context, binding, "hasIntl", has_intl)?;
            Ok(binding)
        }
        "symbols" => super::util_shared_vm::symbols_binding(context),
        "cares_wrap" => crate::host::shared_vm::commonjs::cares_binding(context),
        "http_parser" => super::http_common_shared_vm::module(context),
        "http2" => crate::host::shared_vm::commonjs::http2_binding(context),
        "buffer" => super::buffer_binding_shared_vm::binding(context),
        "url" => super::url_binding_shared_vm::binding(context),
        "blob" => super::blob_binding_shared_vm::binding(context),
        "crypto" => super::tls_shared_vm::facade::binding(context),
        "timers" => super::timers_shared_vm::binding(context),
        "performance" => super::performance_binding_shared_vm::binding(context),
        "uv" => super::util_system_error_shared_vm::uv_binding(context),
        "fs_event_wrap" => super::fs_watch_shared_vm::event_binding(context),
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
                context.host_function(crate::host::shared_vm::operation!("cjsLexerParse"))?;
            put(context, binding, "parse", parse)?;
            Ok(binding)
        }
        "constants" => {
            let binding = context.object_rooted()?;
            let fs = super::fs_shared_vm::fs_constants(context)?;
            put(context, binding, "fs", fs)?;
            #[cfg(unix)]
            {
                let os = super::os_binding_shared_vm::constants(context)?;
                put(context, binding, "os", os)?;
            }
            let zlib = super::zlib_constants::constants(context)?;
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
            Err(context.throw(error))
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
    if quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY {
        return Err(context.native_execution_unavailable("native source analysis required: CJS lexer is unavailable"));
    }
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

/// `tty_wrap`'s handle members: (name, operation, length).
const TTY_METHODS: [(&str, &str, f64); 2] = [("getWindowSize", "ttyWrapGetWindowSize", 1.0), ("setRawMode", "ttyWrapSetRawMode", 1.0)];
/// The status libuv's tty calls report: success.
const UV_OK: f64 = 0.0;

/// Node's `tty_wrap` binding: `isTTY(fd)` as the `tty` module answers it, and a `TTY` handle
/// with no terminal of its own, whose window has no size and whose mode changes succeed.
fn tty_binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    let is_tty = context.host_function_named(crate::host::shared_vm::operation!("ttyIsatty"), "isTTY")?;
    put(context, binding, "isTTY", is_tty)?;
    let class = context.host_constructor_named(crate::host::shared_vm::operation!("ttyWrapConstructor"), "TTY")?;
    let prototype = get(context, class, "prototype")?;
    for (name, operation, length) in TTY_METHODS {
        let function = context.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let length = context.number(length);
        context.define_named_data_property_rooted(function, "length", length, false, false, true)?;
        context.define_named_data_property_rooted(prototype, name, function, true, false, true)?;
    }
    put(context, binding, "TTY", class)?;
    Ok(binding)
}

pub(crate) fn tty_wrap_constructor(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(context.undefined())
}

/// `getWindowSize(size)`: `size` filled with no columns and no rows.
pub(crate) fn tty_wrap_get_window_size(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if let Some(&size) = args.first().filter(|&&size| context.is_object_rooted(size).unwrap_or(false)) {
        for index in 0..2 {
            let key = context.number(f64::from(index));
            let zero = context.number(0.0);
            context.set_property_rooted(size, key, zero, size)?;
        }
    }
    Ok(context.number(UV_OK))
}

pub(crate) fn tty_wrap_set_raw_mode(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(context.number(UV_OK))
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
