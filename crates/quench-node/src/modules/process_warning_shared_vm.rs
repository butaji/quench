//! Node's process warnings: `process.emitWarning` and the default `warning` listener
//! (`lib/internal/process/warning.js`'s semantics), in Rust.

use std::cell::Cell;
use std::io::Write;

use quench_runtime::{NativeContext, RootId, RootedError};

use crate::host::NodeHost;

type Context<'a> = NativeContext<'a, NodeHost>;

/// The name a warning takes without a `type`.
const DEFAULT_TYPE: &str = "Warning";
/// The type whose warnings `--no-deprecation`, `--throw-deprecation` and
/// `--trace-deprecation` govern.
const DEPRECATION: &str = "DeprecationWarning";

thread_local! {
    /// Whether the "(Use `node --trace-warnings ...`)" hint has been printed.
    static TRACE_HINT_SHOWN: Cell<bool> = const { Cell::new(false) };
    /// The features `emit_experimental` has warned about.
    static EXPERIMENTAL_WARNED: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Node's `emitExperimentalWarning(feature)`: one `ExperimentalWarning` per feature.
pub(crate) fn emit_experimental(c: &mut Context<'_>, feature: &'static str) -> Result<(), RootedError> {
    let first = EXPERIMENTAL_WARNED.with_borrow_mut(|warned| {
        let first = !warned.contains(&feature);
        if first {
            warned.push(feature);
        }
        first
    });
    if first {
        let process = process(c)?;
        let emit = get(c, process, "emitWarning")?;
        let message = c.string_rooted(&format!("{feature} is an experimental feature and might change at any time"));
        let kind = c.string_rooted("ExperimentalWarning");
        c.call_rooted(emit, process, &[message, kind])?;
    }
    Ok(())
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn process(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process is not initialized"))
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

/// Ahead of a warning that follows a release of the compiler's pages; nothing to build.
pub(crate) fn prepare(_: &mut Context<'_>) -> Result<(), RootedError> {
    Ok(())
}

/// `process.emitWarning`.
pub(crate) fn emit_warning(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_function_named(crate::host::shared_vm::operation("processEmitWarning"), "emitWarning")
}

/// The default `warning` listener.
pub(crate) fn on_warning(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_function_named(crate::host::shared_vm::operation("processOnWarning"), "onWarning")
}

fn validate_string(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.string_text(value)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, value, name, "string")?);
    }
    Ok(())
}

/// `emitWarning(warning[, type[, code]][, ctor])` or `emitWarning(warning, options)`.
pub(crate) fn emit(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let argument = |index: usize| args.get(index).copied().unwrap_or(undefined);
    let mut warning = argument(0);
    let mut kind = argument(1);
    let mut code = argument(2);
    let mut constructor = argument(3);
    let mut detail = undefined;
    if c.is_object_rooted(kind)? && !c.is_array_rooted(kind)? && !c.is_callable_rooted(kind)? {
        let options = kind;
        constructor = get(c, options, "ctor")?;
        code = get(c, options, "code")?;
        let given = get(c, options, "detail")?;
        if c.string_text(given)?.is_some() {
            detail = given;
        }
        kind = get(c, options, "type")?;
        if !c.truthy_rooted(kind)? {
            kind = c.string_rooted(DEFAULT_TYPE);
        }
    } else if c.is_callable_rooted(kind)? {
        constructor = kind;
        code = undefined;
        kind = c.string_rooted(DEFAULT_TYPE);
    }
    if !is_undefined(c, kind) {
        validate_string(c, kind, "type")?;
    }
    if c.is_callable_rooted(code)? {
        constructor = code;
        code = undefined;
    } else if !is_undefined(c, code) {
        validate_string(c, code, "code")?;
    }
    if let Some(message) = c.string_text(warning)? {
        warning = c.error_rooted(&message)?;
        let name = if c.truthy_rooted(kind)? { c.to_string(kind)? } else { DEFAULT_TYPE.to_owned() };
        let name = c.string_rooted(&name);
        c.assign_named_property_rooted(warning, "name", name)?;
        if !is_undefined(c, code) {
            c.assign_named_property_rooted(warning, "code", code)?;
        }
        if !is_undefined(c, detail) {
            c.assign_named_property_rooted(warning, "detail", detail)?;
        }
        let global = c.global_root()?;
        let error = get(c, global, "Error")?;
        let capture = get(c, error, "captureStackTrace")?;
        if c.is_callable_rooted(capture)? {
            let process = process(c)?;
            let anchor = if c.is_callable_rooted(constructor)? { constructor } else { get(c, process, "emitWarning")? };
            c.call_rooted(capture, error, &[warning, anchor])?;
        }
    } else if !c.is_error_rooted(warning)? {
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, warning, "warning", "of type string or an instance of Error")?);
    }
    let process = process(c)?;
    let name = get(c, warning, "name")?;
    if c.string_text(name)?.as_deref() == Some(DEPRECATION) {
        let silenced = get(c, process, "noDeprecation")?;
        if c.truthy_rooted(silenced)? {
            return Ok(undefined);
        }
        let throws = get(c, process, "throwDeprecation")?;
        if c.truthy_rooted(throws)? {
            return Err(c.throw(warning));
        }
    }
    let dispatch = c.host_function_with_data(crate::host::shared_vm::operation("processEmitWarningEvent"), warning)?;
    super::shared_event_loop::queue_next_tick(c, dispatch, undefined, &[])?;
    Ok(undefined)
}

/// `doEmitWarning(warning)`: `process.emit('warning', warning)`.
pub(crate) fn emit_event(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let warning = c.host_function_data()?;
    let process = process(c)?;
    let emit = get(c, process, "emit")?;
    let event = c.string_rooted("warning");
    c.call_rooted(emit, process, &[event, warning])
}

/// `onWarning(warning)`: `(node:<pid>) [<code>] <name>: <message>`, its detail, and once the
/// hint naming the trace flag, on stderr or the `--redirect-warnings` file.
pub(crate) fn on(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let warning = args.first().copied().unwrap_or(undefined);
    if !c.is_error_rooted(warning)? {
        return Ok(undefined);
    }
    let process = process(c)?;
    let name = get(c, warning, "name")?;
    let deprecation = c.string_text(name)?.as_deref() == Some(DEPRECATION);
    let silenced = get(c, process, "noDeprecation")?;
    if deprecation && c.truthy_rooted(silenced)? {
        return Ok(undefined);
    }
    let trace_warnings = get(c, process, "traceProcessWarnings")?;
    let trace_deprecation = get(c, process, "traceDeprecation")?;
    let trace = c.truthy_rooted(trace_warnings)? || (deprecation && c.truthy_rooted(trace_deprecation)?);
    let release = get(c, process, "release")?;
    let runtime = if c.is_object_rooted(release)? { get(c, release, "name")? } else { undefined };
    let runtime = if c.truthy_rooted(runtime)? { c.to_string(runtime)? } else { "node".to_owned() };
    let pid = get(c, process, "pid")?;
    let pid = c.to_string(pid)?;
    let mut message = format!("({runtime}:{pid}) ");
    let code = get(c, warning, "code")?;
    if c.truthy_rooted(code)? {
        message.push_str(&format!("[{}] ", c.to_string(code)?));
    }
    let stack = get(c, warning, "stack")?;
    if trace && c.truthy_rooted(stack)? {
        message.push_str(&c.to_string(stack)?);
    } else {
        message.push_str(&c.to_string(warning)?);
    }
    let detail = get(c, warning, "detail")?;
    if let Some(detail) = c.string_text(detail)? {
        message.push('\n');
        message.push_str(&detail);
    }
    if !trace && !TRACE_HINT_SHOWN.get() {
        TRACE_HINT_SHOWN.set(true);
        let flag = if deprecation { "--trace-deprecation" } else { "--trace-warnings" };
        let argv0 = get(c, process, "argv0")?;
        let argv0 = if c.truthy_rooted(argv0)? { c.to_string(argv0)? } else { "node".to_owned() };
        message.push_str(&format!("\n(Use `{} {flag} ...` to show where the warning was created)", basename(&argv0, ".exe")));
    }
    let redirect = c.host_mut().shared_state().borrow().cli_options.string_value("--redirect-warnings").map(str::to_owned);
    match redirect {
        Some(path) if !path.is_empty() => {
            let written = std::fs::OpenOptions::new().create(true).append(true).open(&path)
                .and_then(|mut file| writeln!(file, "{message}"));
            if written.is_err() {
                eprintln!("{message}");
            }
        }
        _ => eprintln!("{message}"),
    }
    Ok(undefined)
}

/// `path.posix.basename(path, suffix)` (`lib/path.js`) over UTF-8 text. The module passes
/// an ASCII suffix (`'.exe'`), which matches byte by byte from the end exactly where it
/// matches code unit by code unit.
fn basename<'a>(path: &'a str, suffix: &str) -> &'a str {
    let bytes = path.as_bytes();
    let suffix = suffix.as_bytes();
    let separator = b'/';
    let mut start = 0;
    let mut matched_slash = true;
    if !suffix.is_empty() && suffix.len() <= bytes.len() {
        if suffix == bytes {
            return "";
        }
        let mut suffix_index = Some(suffix.len() - 1);
        let mut end = None;
        let mut first_non_slash_end = None;
        for index in (0..bytes.len()).rev() {
            let byte = bytes[index];
            if byte == separator {
                if !matched_slash {
                    start = index + 1;
                    break;
                }
                continue;
            }
            if first_non_slash_end.is_none() {
                matched_slash = false;
                first_non_slash_end = Some(index + 1);
            }
            if let Some(position) = suffix_index {
                if byte == suffix[position] {
                    suffix_index = position.checked_sub(1);
                    if suffix_index.is_none() {
                        end = Some(index);
                    }
                } else {
                    suffix_index = None;
                    end = first_non_slash_end;
                }
            }
        }
        let end = match end {
            Some(end) if end == start => first_non_slash_end,
            Some(end) => Some(end),
            None => Some(bytes.len()),
        };
        return end.map_or("", |end| &path[start..end]);
    }
    let mut end = None;
    for index in (0..bytes.len()).rev() {
        if bytes[index] == separator {
            if !matched_slash {
                start = index + 1;
                break;
            }
        } else if end.is_none() {
            matched_slash = false;
            end = Some(index + 1);
        }
    }
    end.map_or("", |end| &path[start..end])
}

#[cfg(test)]
mod tests {
    use super::basename;

    #[test]
    fn basename_is_node_posix_basename() {
        for (path, expected) in [
            ("/usr/bin/node", "node"),
            ("node", "node"),
            ("/opt/node.exe", "node"),
            ("node.exe/", "node"),
            ("/a/b/", "b"),
            (".exe", ""),
            ("x.exe.exe", "x.exe"),
            ("/", ""),
            ("", ""),
            ("a/.exe", ".exe"),
            ("quench-node", "quench-node"),
        ] {
            assert_eq!(basename(path, ".exe"), expected, "{path}");
        }
    }
}
