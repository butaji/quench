//! `util.debuglog` (lib/internal/util/debuglog.js): loggers a `NODE_DEBUG` pattern enables.
use super::util_module_shared_vm::{argument, get, process, put, type_of, Context};
use crate::modules::util_inspect_shared_vm as inspect;
use quench_runtime::{RootId, RootedError, ValueType};

/// `util.debuglog(set, cb)`: a logger enabled by `NODE_DEBUG`.
pub(crate) fn debuglog(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let set = argument(c, args, 0);
    let callback = argument(c, args, 1);
    let state = c.null_object_rooted()?;
    put(c, state, "set", set)?;
    put(c, state, "callback", callback)?;
    let logger = c.host_function_named_with_data(crate::host::shared_vm::operation!("utilDebugLogger"), "logger", state)?;
    let getter = c.host_function_named_with_data(crate::host::shared_vm::operation!("utilDebugEnabled"), "get", state)?;
    let descriptor = c.null_object_rooted()?;
    put(c, descriptor, "get", getter)?;
    let yes = c.boolean(true);
    put(c, descriptor, "enumerable", yes)?;
    put(c, descriptor, "configurable", yes)?;
    let key = c.string_rooted("enabled");
    c.define_property_descriptor_rooted(logger, key, descriptor)?;
    Ok(logger)
}

/// Whether `NODE_DEBUG` (a comma list of case-insensitive `*` patterns) names `set`.
fn debug_enabled(set: &str) -> bool {
    let Ok(patterns) = std::env::var("NODE_DEBUG") else { return false };
    patterns.split(',').any(|pattern| wildcard_match(&pattern.to_uppercase(), set))
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, tail)) => {
            text.starts_with(head)
                && (head.len()..=text.len()).any(|start| text.is_char_boundary(start) && wildcard_match(tail, &text[start..]))
        }
    }
}

fn debug_set(c: &mut Context<'_>, state: RootId) -> Result<String, RootedError> {
    let set = get(c, state, "set")?;
    Ok(c.to_string(set)?.to_uppercase())
}

pub(crate) fn debug_enabled_get(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let set = debug_set(c, state)?;
    Ok(c.boolean(debug_enabled(&set)))
}

/// The logger: on its first call it resolves the set's implementation and hands it to the
/// callback; an enabled set writes `SET pid: message` to stderr.
pub(crate) fn debug_logger(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let set = debug_set(c, state)?;
    let implementations = inspect::primordial(c, "debugImpls")?;
    let mut implementation = get(c, implementations, &set)?;
    if type_of(c, implementation) == ValueType::Undefined {
        let set_root = c.string_rooted(&set);
        implementation = if debug_enabled(&set) {
            c.host_function_named_with_data(crate::host::shared_vm::operation!("utilDebugWrite"), "debug", set_root)?
        } else {
            c.host_function_named(crate::host::shared_vm::operation!("utilNoop"), "noop")?
        };
        put(c, implementations, &set, implementation)?;
    }
    let initialized = get(c, state, "initialized")?;
    if !c.truthy_rooted(initialized)? {
        let yes = c.boolean(true);
        put(c, state, "initialized", yes)?;
        let callback = get(c, state, "callback")?;
        if c.is_callable_rooted(callback)? {
            let undefined = c.undefined();
            c.call_rooted(callback, undefined, &[implementation])?;
        }
    }
    let undefined = c.undefined();
    c.call_rooted(implementation, undefined, args)
}

pub(crate) fn noop(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}

/// An enabled set's `debug(...args)`.
pub(crate) fn debug_write(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let set = c.host_function_data()?;
    let set = c.to_string(set)?;
    let process = process(c)?;
    let stderr = get(c, process, "stderr")?;
    let colors = super::util_style_shared_vm::should_colorize(c, stderr)?;
    let options = c.object_rooted()?;
    let colors_root = c.boolean(colors);
    put(c, options, "colors", colors_root)?;
    let message = inspect::format_with_options(c, Some(options), args)?;
    let pid = get(c, process, "pid")?;
    let pid = inspect::format_with_options(c, Some(options), &[pid])?;
    let mut line = format!("{set} ").encode_utf16().collect::<Vec<_>>();
    line.extend(pid);
    line.extend(": ".encode_utf16());
    line.extend(message);
    line.push(u16::from(b'\n'));
    let line = c.string_units_rooted(&line);
    let write = get(c, stderr, "write")?;
    c.call_rooted(write, stderr, &[line])?;
    Ok(c.undefined())
}
