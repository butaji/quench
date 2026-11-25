//! `internalBinding(name)`: the host's native bindings. A binding a program can reach only
//! through an optional family is a host operation (`shared_vm::call_linked`), so an image that
//! cannot reach it links `unlinked_operation` in its place and keeps the module, and the native
//! library it uses, out of the executable.

use super::{get, put};
use crate::host::shared_vm::{call_linked, marked_operation, MarkedOperation};
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a>;

/// The bindings routed through host operations (`internalBinding:NAME`): each module owns its
/// builder, and the linker keeps it only for the operation families the program reaches.
const OPTIONAL_BINDINGS: &[(&str, MarkedOperation)] = &[
    ("crypto", marked_operation!("internalBinding:crypto")),
    ("crypto_keys", marked_operation!("internalBinding:crypto_keys")),
    ("zlib", marked_operation!("internalBinding:zlib")),
    ("cares_wrap", marked_operation!("internalBinding:cares_wrap")),
    ("http_parser", marked_operation!("internalBinding:http_parser")),
    ("http2", marked_operation!("internalBinding:http2")),
    ("udp_wrap", marked_operation!("internalBinding:udp_wrap")),
    ("tcp_wrap", marked_operation!("internalBinding:tcp_wrap")),
    ("pipe_wrap", marked_operation!("internalBinding:pipe_wrap")),
    ("stream_wrap", marked_operation!("internalBinding:stream_wrap")),
    ("contextify", marked_operation!("internalBinding:contextify")),
    ("v8", marked_operation!("internalBinding:v8")),
    ("heap_utils", marked_operation!("internalBinding:heap_utils")),
    ("internal_only_v8", marked_operation!("internalBinding:internal_only_v8")),
    ("performance", marked_operation!("internalBinding:performance")),
];

/// The binding `name` names, or `None` when only required bindings remain.
fn optional(context: &mut Context<'_>, name: &str) -> Result<Option<RootId>, RootedError> {
    let Some((_, operation)) = OPTIONAL_BINDINGS.iter().find(|(candidate, _)| *candidate == name) else {
        return Ok(None);
    };
    call_linked(context, operation.id(), &[])
}

/// The binding `name` names: a required one, or an optional one this image links.
pub(super) fn build_internal_binding(context: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    if let Some(binding) = optional(context, name)? {
        return Ok(binding);
    }
    match name {
        #[cfg(unix)]
        "os" | "credentials" | "process_methods" => platform_binding(context, name),
        "trace_events" | "cjs_lexer" | "constants" => protocol_binding(context, name),
        _ => host_binding(context, name),
    }
}

/// A binding that is a builder with no per-image state of its own.
type Builder = fn(&mut Context<'_>) -> Result<RootId, RootedError>;

/// The required bindings whose builder is the module's own function: one table, one lookup.
const HOST_BINDINGS: &[(&str, Builder)] = &[
    ("fs", super::super::fs_directory_shared_vm::binding),
    ("errors", super::super::node_errors_shared_vm::binding),
    ("report", super::super::report_configuration_shared_vm::binding),
    ("types", super::super::util_shared_vm::types_binding),
    ("messaging", super::super::messaging_binding_shared_vm::binding),
    ("util", super::super::util_shared_vm::key_binding),
    ("options", super::super::cli_options_shared_vm::binding),
    ("string_decoder", super::super::string_decoder_shared_vm::binding),
    ("serdes", super::super::serdes_shared_vm::binding),
    ("ipc_serdes", super::super::ipc_serdes_shared_vm::binding),
    ("encoding_binding", super::super::encoding_binding_shared_vm::binding),
    #[cfg(quench_icu)]
    ("icu", super::super::icu_binding_shared_vm::binding),
    ("symbols", super::super::util_shared_vm::symbols_binding),
    ("buffer", super::super::buffer_binding_shared_vm::binding),
    ("url", super::super::url_binding_shared_vm::binding),
    ("blob", super::super::blob_binding_shared_vm::binding),
    ("timers", super::super::timers_shared_vm::binding),
    ("uv", super::super::util_system_error_shared_vm::uv_binding),
    ("fs_event_wrap", super::super::fs_watch_shared_vm::event_binding),
];

/// A required binding whose arm has a body of its own, or a builder from `HOST_BINDINGS`.
fn host_binding(context: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    match name {
        "modules" => crate::host::shared_vm::commonjs::cached_native_binding(
            context, "\0binding:modules", module_protocol_binding,
        ),
        "tty_wrap" => tty_binding(context),
        "config" => {
            let binding = context.object_rooted()?;
            let has_intl = context.boolean(cfg!(quench_icu));
            put(context, binding, "hasIntl", has_intl)?;
            Ok(binding)
        }
        _ => match HOST_BINDINGS.iter().find(|(candidate, _)| *candidate == name) {
            Some((_, build)) => build(context),
            None => no_such_binding(context, name),
        },
    }
}

#[cfg(unix)]
fn platform_binding(context: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    match name {
        "os" => super::super::os_binding_shared_vm::os_binding(context),
        // Node's `lib/os.js` reads `getTempDir` from this binding at load time.
        "credentials" => super::super::os_binding_shared_vm::credentials_binding(context),
        _ => super::super::process_metrics_shared_vm::binding(context),
    }
}

fn protocol_binding(context: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    match name {
        "trace_events" => trace_events_binding(context),
        "cjs_lexer" => cjs_lexer_binding(context),
        _ => constants_binding(context),
    }
}

fn no_such_binding(context: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    let error = context.error_rooted(&format!("No such binding: {name}"))?;
    let code = context.string_rooted("ERR_INVALID_MODULE");
    put(context, error, "code", code)?;
    Err(context.throw(error))
}

/// `constants`: only the sub-objects the program reaches are built (`constants.zlib`,
/// `constants.crypto`).
fn constants_binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    let fs = super::super::fs_shared_vm::fs_constants(context)?;
    put(context, binding, "fs", fs)?;
    #[cfg(unix)]
    {
        let os = super::super::os_binding_shared_vm::constants(context)?;
        put(context, binding, "os", os)?;
    }
    for (name, operation) in [
        ("zlib", crate::host::shared_vm::operation!("internalBinding:constants.zlib")),
        ("crypto", crate::host::shared_vm::operation!("internalBinding:constants.crypto")),
    ] {
        if let Some(constants) = call_linked(context, operation, &[])? {
            put(context, binding, name, constants)?;
        }
    }
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

fn trace_events_binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    for (name, operation) in [
        ("getCategoryEnabledBuffer", crate::host::shared_vm::operation!("traceCategoryBuffer")),
        ("trace", crate::host::shared_vm::operation!("traceRecord")),
        ("isTraceCategoryEnabled", crate::host::shared_vm::operation!("traceCategoryEnabled")),
        ("getEnabledCategories", crate::host::shared_vm::operation!("traceGetEnabledCategories")),
    ] {
        let function = context.host_function(operation)?;
        put(context, binding, name, function)?;
    }
    let perfetto = context.boolean(false);
    put(context, binding, "usePerfetto", perfetto)?;
    Ok(binding)
}

fn cjs_lexer_binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    let parse = context.host_function(crate::host::shared_vm::operation!("cjsLexerParse"))?;
    put(context, binding, "parse", parse)?;
    Ok(binding)
}

/// `tty_wrap`'s handle members: (name, operation, length).
const TTY_METHODS: [(&str, MarkedOperation, f64); 2] = [("getWindowSize", marked_operation!("ttyWrapGetWindowSize"), 1.0), ("setRawMode", marked_operation!("ttyWrapSetRawMode"), 1.0)];
/// The status libuv's tty calls report: success.
const UV_OK: f64 = 0.0;
/// `getWindowSize`'s fields: the columns and the rows.
const TTY_SIZE_FIELDS: u32 = 2;

/// Node's `tty_wrap` binding: `isTTY(fd)` as the `tty` module answers it, and a `TTY` handle
/// with no terminal of its own, whose window has no size and whose mode changes succeed.
fn tty_binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    let is_tty = context.host_function_named(crate::host::shared_vm::operation!("ttyIsatty"), "isTTY")?;
    put(context, binding, "isTTY", is_tty)?;
    let class = context.host_constructor_named(crate::host::shared_vm::operation!("ttyWrapConstructor"), "TTY")?;
    let prototype = get(context, class, "prototype")?;
    for (name, operation, length) in TTY_METHODS {
        let function = context.host_function_named(operation.id(), name)?;
        let length = context.number(length);
        context.define_named_data_property_rooted(function, "length", length, false, false, true)?;
        context.define_named_data_property_rooted(prototype, name, function, true, false, true)?;
    }
    put(context, binding, "TTY", class)?;
    Ok(binding)
}

pub(crate) fn tty_wrap_constructor(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(context.undefined())
}

/// `getWindowSize(size)`: `size` filled with no columns and no rows.
pub(crate) fn tty_wrap_get_window_size(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if let Some(&size) = args.first().filter(|&&size| context.is_object_rooted(size).unwrap_or(false)) {
        for index in 0..TTY_SIZE_FIELDS {
            let key = context.number(f64::from(index));
            let zero = context.number(0.0);
            context.set_property_rooted(size, key, zero, size)?;
        }
    }
    Ok(context.number(UV_OK))
}

pub(crate) fn tty_wrap_set_raw_mode(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(context.number(UV_OK))
}

fn module_protocol_binding(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    let statuses = super::super::node_protocol_constants::COMPILE_CACHE_STATUS
        .iter()
        .map(|status| context.string_rooted(status))
        .collect::<Vec<_>>();
    let statuses = context.array_rooted(&statuses)?;
    put(context, binding, "compileCacheStatus", statuses)?;
    let types = context.null_object_rooted()?;
    for &(name, value) in super::super::node_protocol_constants::CACHED_CODE_TYPES {
        let value = context.number(f64::from(value));
        put(context, types, name, value)?;
    }
    put(context, binding, "cachedCodeTypes", types)?;
    Ok(binding)
}
