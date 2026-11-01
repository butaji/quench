//! Public Module search-path projections over the canonical path and loader operations.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use super::event_constructor_shared_vm::get;
type Context<'a> = NativeContext<'a, NodeHost>;

pub(crate) fn install(c: &mut Context<'_>, module: RootId, process: RootId, path: RootId, delimiter: RootId) -> Result<(), RootedError> {
    let data = c.record_rooted(&["module", "process", "path", "delimiter"], &[module, process, path, delimiter])?;
    for (name, operation) in [("_initPaths", "moduleInitPaths"), ("_resolveLookupPaths", "moduleLookupPaths"), ("_load", "moduleLoadFacade")] {
        let function = c.host_constructor_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
        c.assign_named_property_rooted(module, name, function)?;
    }
    Ok(())
}

fn invoke(c: &mut Context<'_>, object: RootId, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = get(c, object, name)?;
    c.call_rooted(function, object, args)
}

fn nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null() || value.is_undefined())
}

fn spread(c: &mut Context<'_>, source: RootId, values: &mut Vec<RootId>) -> Result<(), RootedError> {
    let iterator = c.iterator_rooted(source)?;
    while let Some(value) = c.iterator_step_rooted(&iterator)? { values.push(value); }
    Ok(())
}

pub(crate) fn initialize(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let process = get(c, data, "process")?;
    let env = get(c, process, "env")?;
    let home = get(c, env, "HOME")?;
    let env = get(c, process, "env")?;
    let node_path = get(c, env, "NODE_PATH")?;
    let path = get(c, data, "path")?;
    let executable = get(c, process, "execPath")?;
    let parent = c.string_rooted("..");
    let lib = c.string_rooted("lib");
    let node = c.string_rooted("node");
    let prefix = invoke(c, path, "resolve", &[executable, parent, parent, lib, node])?;
    let mut paths = Vec::new();
    if c.truthy_rooted(home)? {
        for name in [".node_modules", ".node_libraries"] {
            let name = c.string_rooted(name);
            paths.push(invoke(c, path, "resolve", &[home, name])?);
        }
    }
    paths.push(prefix);
    if c.truthy_rooted(node_path)? {
        let delimiter = get(c, data, "delimiter")?;
        let parts = invoke(c, node_path, "split", &[delimiter])?;
        let mut requested = Vec::new();
        for part in c.array_like_values_rooted(parts)? { if c.truthy_rooted(part)? { requested.push(part); } }
        requested.extend(paths);
        paths = requested;
    }
    let paths = c.array_rooted(&paths)?;
    let module = get(c, data, "module")?;
    c.assign_named_property_rooted(module, "globalPaths", paths)?;
    Ok(c.undefined())
}

pub(crate) fn lookup(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let request = args.first().copied().unwrap_or(undefined);
    let parent = args.get(1).copied().unwrap_or(undefined);
    let global = c.global_root()?;
    let string = get(c, global, "String")?;
    let request = c.call_rooted(string, undefined, &[request])?;
    let text = c.to_string(request)?;
    let data = c.host_function_data()?;
    let path = get(c, data, "path")?;
    let separator = get(c, path, "sep")?;
    let windows = c.string_text(separator)?.as_deref() == Some("\\");
    let relative = text.starts_with('.') && (text.len() == 1 || matches!(text.as_bytes().get(1), Some(b'.' | b'/')) || (windows && text.as_bytes().get(1) == Some(&b'\\')));
    let module = get(c, data, "module")?;
    if !relative {
        let builtins = get(c, module, "builtinModules")?;
        let included = invoke(c, builtins, "includes", &[request])?;
        let builtin = if c.truthy_rooted(included)? { true } else {
            let plain = c.string_rooted(text.strip_prefix("node:").unwrap_or(&text));
            let included = invoke(c, builtins, "includes", &[plain])?;
            c.truthy_rooted(included)?
        };
        if builtin { return Ok(c.null()); }
        let mut values = Vec::new();
        if !nullish(c, parent) {
            let paths = get(c, parent, "paths")?;
            if !nullish(c, paths) {
                let length = get(c, paths, "length")?;
                if c.truthy_rooted(length)? {
                    let paths = get(c, parent, "paths")?;
                    spread(c, paths, &mut values)?;
                }
            }
        }
        let paths = get(c, module, "globalPaths")?;
        spread(c, paths, &mut values)?;
        return if values.is_empty() { Ok(c.null()) } else { c.array_rooted(&values) };
    }
    if c.truthy_rooted(parent)? {
        let id = get(c, parent, "id")?;
        if c.truthy_rooted(id)? {
            let filename = get(c, parent, "filename")?;
            if c.truthy_rooted(filename)? {
                let filename = get(c, parent, "filename")?;
                let directory = invoke(c, path, "dirname", &[filename])?;
                return c.array_rooted(&[directory]);
            }
        }
    }
    let current = c.string_rooted(".");
    c.array_rooted(&[current])
}

pub(crate) fn load(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let request = args.first().copied().unwrap_or(undefined);
    let parent = args.get(1).copied().unwrap_or(undefined);
    if c.truthy_rooted(parent)? {
        let require = get(c, parent, "require")?;
        if c.is_callable_rooted(require)? { return invoke(c, parent, "require", &[request]); }
    }
    let global = c.global_root()?;
    let string = get(c, global, "String")?;
    let request = c.call_rooted(string, undefined, &[request])?;
    let data = c.host_function_data()?;
    let module = get(c, data, "module")?;
    let process = get(c, data, "process")?;
    let builtins = get(c, module, "builtinModules")?;
    let included = invoke(c, builtins, "includes", &[request])?;
    if c.truthy_rooted(included)? {
        let text = c.to_string(request)?;
        let name = c.string_rooted(text.strip_prefix("node:").unwrap_or(&text));
        return invoke(c, process, "getBuiltinModule", &[name]);
    }
    let path = get(c, data, "path")?;
    let cwd = invoke(c, process, "cwd", &[])?;
    let filename = c.string_rooted("__quench_module_load__.js");
    let entry = invoke(c, path, "resolve", &[cwd, filename])?;
    let require = invoke(c, module, "createRequire", &[entry])?;
    c.call_rooted(require, undefined, &[request])
}
