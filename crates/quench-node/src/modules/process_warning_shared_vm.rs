//! Node's process warnings: `process.emitWarning` and the default `warning` listener.
//! Their JavaScript implementation was removed (decision 38); they await a Rust port.

use quench_runtime::{NativeContext, RootId, RootedError};

use crate::host::shared_vm::commonjs;
use crate::host::NodeHost;


/// Node's warning module, relative to the library's `lib`.
const WARNING_MODULE: &str = "internal/process/warning.js";


/// The `[emitWarning, onWarning]` pair, built once.
fn binding(_: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    crate::host::removed_javascript(format!("node:{WARNING_MODULE}"))
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
