//! Node's process warnings without Node's warning module's imports.
//!
//! `process.emitWarning` and the default `warning` listener are Node's own `emitWarning` and
//! `onWarning`: `lib/internal/process/warning.js` runs unchanged, as a library module does,
//! but with its own `require` and `primordials` (`PRELUDE`). Loading it as a library module
//! requires `internal/errors`, `internal/validators`, `internal/options`, `internal/assert`
//! and the primordials, and its `writeOut` loads the SDK console: several megabytes to print
//! one line. Here its imports are the primordials it uses, from the startup intrinsics;
//! `getOptionValue` over the parsed options; a validator, assertion and error constructor
//! that load Node's own modules on the paths that throw; `path.basename`; the console's
//! `error` (`console_shared_vm::warning_writer`); and the public builtins. Scripts that can
//! reach Node's module (`--expose-internals`) use the library module itself.

use quench_runtime::{NativeContext, RootId, RootedError};

use crate::host::shared_vm::commonjs;
use crate::host::NodeHost;

/// The cached `[emitWarning, onWarning]` pair.
const BINDING: &str = "\0binding:process-warning";

/// Node's warning module, relative to the library's `lib`.
const WARNING_MODULE: &str = "internal/process/warning.js";

/// The hidden global that requires the public builtins (`commonjs::initialize`).
const BUILTIN_REQUIRE: &str = crate::host::shared_vm::commonjs::BOOTSTRAP_REQUIRE;

/// The module's `require` and `primordials`, built over the startup intrinsics (captured as
/// Node's primordials are) from `library(id)` (one of Node's library modules), `builtin(id)`
/// (a public builtin), `option(name)` (`getOptionValue`), `basename(path, suffix)` and
/// `consoleError()`.
/// * `SafeSet` keeps its entries in a set through the original `add` and `has`, so changes
///   to `Set.prototype` cannot reach it, as `makeSafe` ensures; the module uses only those.
/// * `validateString` is `hideStackFrames`-wrapped in Node: a failure's stack starts at its
///   caller, as it does here.
/// * `ERR_INVALID_ARG_TYPE` constructs Node's error, whose stack starts at its caller.
/// * `isErrorStackTraceLimitWritable` is `internal/errors.js`'s, outside a snapshot build.
/// * The console's `error` is read when `writeOut` first writes, as Node reads it.
const PRELUDE: &str = r#"
  const call = Function.prototype.call.bind(Function.prototype.call);
  const NativeError = Error;
  const NativeSet = Set;
  const { captureStackTrace } = NativeError;
  const errorToString = NativeError.prototype.toString;
  const { getOwnPropertyDescriptor, isExtensible } = Object;
  const hasOwn = Object.prototype.hasOwnProperty;
  const setAdd = NativeSet.prototype.add;
  const setHas = NativeSet.prototype.has;
  function SafeSet() { this.set = new NativeSet(); }
  SafeSet.prototype = { __proto__: null, add(value) { call(setAdd, this.set, value); return this; }, has(value) { return call(setHas, this.set, value); } };
  const primordials = {
    __proto__: null,
    ArrayIsArray: Array.isArray,
    Error: NativeError,
    ErrorCaptureStackTrace: captureStackTrace,
    ErrorPrototypeToString: (error) => call(errorToString, error),
    SafeSet,
    String,
  };
  function assert(value, message) {
    if (!value) library('internal/assert')(value, message);
  }
  function ERR_INVALID_ARG_TYPE(...args) {
    const error = new (library('internal/errors').codes.ERR_INVALID_ARG_TYPE)(...args);
    captureStackTrace(error, ERR_INVALID_ARG_TYPE);
    return error;
  }
  function validateString(value, name) {
    if (typeof value === 'string') return;
    try {
      library('internal/validators').validateString(value, name);
    } catch (error) {
      captureStackTrace(error, validateString);
      throw error;
    }
  }
  function isErrorStackTraceLimitWritable() {
    const desc = getOwnPropertyDescriptor(NativeError, 'stackTraceLimit');
    if (desc === undefined) return isExtensible(NativeError);
    return call(hasOwn, desc, 'writable') ? desc.writable : desc.set !== undefined;
  }
  const modules = {
    __proto__: null,
    'internal/options': { getOptionValue: option },
    'internal/assert': assert,
    'internal/errors': { codes: { ERR_INVALID_ARG_TYPE }, isErrorStackTraceLimitWritable },
    'internal/validators': { validateString },
    'path': { basename },
  };
  const require = (id) => id === 'internal/console/global' ? { error: consoleError() } : id in modules ? modules[id] : builtin(id);
  const module = { exports: {} };
"#;

/// The `[emitWarning, onWarning]` pair, built once.
fn binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    commonjs::cached_native_binding(context, BINDING, |context| {
        let process = context
            .host_mut()
            .shared_state()
            .borrow()
            .process_module
            .ok_or_else(|| RootedError::host("process is not initialized"))?;
        let root = context.host_mut().node_library_root.clone();
        let filename = root.join("lib").join(WARNING_MODULE);
        let source = std::fs::read_to_string(&filename)
            .or_else(|error| crate::node_sources::source(&filename, &root).map(str::to_owned).ok_or(error))
            .map_err(|error| RootedError::host(error.to_string()))?;
        // The prelude shares the wrapper's line, so the module keeps its positions.
        let prelude: Vec<&str> = PRELUDE.lines().map(str::trim).collect();
        let prefix = format!(
            "((process, library, builtin, option, basename, consoleError) => {{ 'use strict'; {} ((exports, require, module, primordials) => {{",
            prelude.join(" "),
        );
        let suffix = "\n})(module.exports, require, module, primordials); return [module.exports.emitWarning, module.exports.onWarning]; })";
        let (line, column) = quench_runtime::wrapper_origin(&prefix)
            .ok_or_else(|| RootedError::host("warning module wrapper origin exceeds compiler positions"))?;
        let global = context.global_root()?;
        let key = context.string_rooted(BUILTIN_REQUIRE);
        let builtin = context.get_property_rooted(global, key)?;
        context.with_startup_intrinsics(|context| {
            let factory = context.evaluate_specialized_script_with_origin_rooted(
                &[prefix.as_str(), &source, suffix].concat(),
                &filename.to_string_lossy(),
                line,
                column,
            )?;
            let operation = crate::host::shared_vm::operation;
            let library = context.host_function(operation("processWarningLibrary"))?;
            let option = context.host_function(operation("processWarningOption"))?;
            let basename = context.host_function(operation("processWarningBasename"))?;
            let console_error = context.host_function(operation("consoleWarningWriter"))?;
            let undefined = context.undefined();
            context.call_rooted(factory, undefined, &[process, library, builtin, option, basename, console_error])
        })
    })
}

/// Builds the functions ahead of a warning that follows a release of the compiler's pages
/// (`esm_wasm`: compiling the Wasm module releases them, and its warning comes next).
pub(crate) fn prepare(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    binding(context).map(drop)
}

fn member(context: &mut NativeContext<'_, NodeHost>, index: usize) -> Result<RootId, RootedError> {
    let pair = binding(context)?;
    let key = context.string_rooted(&index.to_string());
    context.get_property_rooted(pair, key)
}

/// `process.emitWarning`: Node's `emitWarning`.
pub(crate) fn emit_warning(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    member(context, 0)
}

/// The default `warning` listener's implementation: Node's `onWarning`.
pub(crate) fn on_warning(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    member(context, 1)
}

fn string_argument(context: &mut NativeContext<'_, NodeHost>, args: &[RootId], index: usize) -> Result<String, RootedError> {
    let undefined = context.undefined();
    context.to_string(args.get(index).copied().unwrap_or(undefined))
}

/// `library(id)`: Node's library module `id`, loaded as the warning module would require it.
pub(crate) fn library(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = string_argument(context, args, 0)?;
    commonjs::node_library_module(context, WARNING_MODULE, &format!("{id}.js"))
}

/// `getOptionValue(name)` for the options the module reads.
pub(crate) fn option(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let name = string_argument(context, args, 0)?;
    let (string, repeated) = {
        let state = context.host_mut().shared_state();
        let state = state.borrow();
        let options = &state.cli_options;
        let repeated = options.repeated_values().find(|(option, _)| *option == name).map(|(_, values)| values.to_vec());
        (options.string_value(&name).map(str::to_owned), repeated)
    };
    match (string, repeated) {
        (Some(value), _) => Ok(context.string_rooted(&value)),
        (None, Some(values)) => {
            let values: Vec<RootId> = values.iter().map(|value| context.string_rooted(value)).collect();
            context.array_rooted(&values)
        }
        (None, None) => Err(RootedError::host(format!("the warning module reads no option {name}"))),
    }
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

/// `require('path').basename(path, suffix)`.
pub(crate) fn path_basename(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let path = string_argument(context, args, 0)?;
    let suffix = string_argument(context, args, 1)?;
    Ok(context.string_rooted(basename(&path, &suffix)))
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
