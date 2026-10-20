//! WebAssembly modules in the ES module graph, as Node's `wasm` translator
//! (`lib/internal/modules/esm/translators.js`) and `createDynamicModule` define them: a
//! `.wasm` file is a module record that imports each of its import modules' namespaces,
//! instantiates itself with them when it evaluates, and binds each export of the instance
//! (an exported global's value, not its `WebAssembly.Global`). An imported Wasm module's
//! global is passed as its `WebAssembly.Global`, as Node does.
//!
//! The record is a generated facade, like the host's other interop wrappers. Its evaluation
//! calls a hidden global (`INSTANTIATE`) that reads the file, compiles it with the
//! `WebAssembly` namespace and instantiates it, warning once that the feature is
//! experimental.
//!
//! Node compiles and checks the module when it translates it, before any module of the graph
//! evaluates; the facade throws the same errors when it evaluates. A module that does not
//! parse has a facade without imports or exports whose compilation throws the
//! `CompileError`, and a reserved import or export name throws its `LinkError` once the
//! module compiles.

use quench_runtime::{NativeContext, RootId, RootedError};
use serde_json::json;
use quench_wasm::ExternKind;

use super::commonjs;
use crate::host::node_host::NodeHost;

/// The hidden global the facades call.
pub(crate) const INSTANTIATE: &str = "\0quench:wasmInstantiate";

/// Node's `emitExperimentalWarning` feature name for importing Wasm module instances, and
/// the warning's type.
const FEATURE: &str = "Importing WebAssembly module instances";
const EXPERIMENTAL_WARNING: &str = "ExperimentalWarning";

/// The module's imports and exports as its facade needs them.
#[derive(Default)]
struct Interface {
    /// Each import module, once, in first-import order (Node's `importsList`).
    modules: Vec<String>,
    /// The imported globals' module and name (Node's `wasmGlobalImports`).
    global_imports: Vec<(String, String)>,
    /// Each export name, once, in order, and whether it names a global.
    exports: Vec<(String, bool)>,
    /// The `LinkError` message of a reserved name, which Node throws once the module compiles.
    link_error: Option<String>,
}

/// Prefixes the ESM integration reserves (https://webassembly.github.io/esm-integration/js-api/index.html#parse-a-webassembly-module).
const RESERVED_MODULE_PREFIX: &str = "wasm-js:";
const RESERVED_NAME_PREFIXES: [&str; 2] = ["wasm:", "wasm-js:"];

/// The interface of the module's bytes. Bytes that do not parse have the empty interface:
/// their compilation reports them.
fn interface(url: &str, bytes: &[u8]) -> Interface {
    let Some(module) = quench_wasm::interface(bytes) else { return Interface::default() };
    let mut interface = Interface::default();
    for import in &module.imports {
        if import.module.starts_with(RESERVED_MODULE_PREFIX) {
            interface.link_error.get_or_insert(format!("Invalid Wasm import \"{}\" in {url}", import.module));
        }
        if RESERVED_NAME_PREFIXES.iter().any(|prefix| import.name.starts_with(prefix)) {
            interface.link_error.get_or_insert(format!("Invalid Wasm import name \"{}\" in {url}", import.name));
        }
        if import.kind == ExternKind::Global {
            interface.global_imports.push((import.module.clone(), import.name.clone()));
        }
        if !interface.modules.contains(&import.module) {
            interface.modules.push(import.module.clone());
        }
    }
    for export in &module.exports {
        if RESERVED_NAME_PREFIXES.iter().any(|prefix| export.name.starts_with(prefix)) {
            interface.link_error.get_or_insert(format!("Invalid Wasm export name \"{}\" in {url}", export.name));
        }
        interface.exports.push((export.name.clone(), export.kind == ExternKind::Global));
    }
    if interface.link_error.is_some() {
        // Node rejects the module before it links, so none of its import modules resolve.
        interface.modules.clear();
    }
    interface
}

/// The facade module text for the Wasm module at `path` whose record is `name`: the path,
/// or the file URL whose query or fragment is part of the module's identity.
pub(crate) fn facade(name: &str, path: &std::path::Path, bytes: &[u8]) -> Result<String, String> {
    // The URL that resolves to the record itself.
    let url = if name.starts_with("file:") {
        name.to_owned()
    } else {
        url::Url::from_file_path(path).map_err(|_| "invalid Wasm module path".to_owned())?.to_string()
    };
    let interface = interface(&url, bytes);
    // The facade's own namespace identifies its instance to modules that import it.
    let mut source = format!("import * as $self from {};\n", json!(url));
    let mut imports = String::new();
    for (index, module) in interface.modules.iter().enumerate() {
        source.push_str(&format!("import * as $import_{index} from {};\n", json!(module)));
        imports.push_str(&format!("[{}]: $import_{index}, ", json!(module)));
    }
    for (index, (name, _)) in interface.exports.iter().enumerate() {
        source.push_str(&format!("let $export_{index};\nexport {{ $export_{index} as {} }};\n", json!(name)));
    }
    source.push_str(&format!(
        "const $exports = globalThis[{}]({}, $self, {{ __proto__: null, {imports}}}, {}, {}, {});\n",
        json!(INSTANTIATE),
        json!(path.to_string_lossy()),
        json!(interface.global_imports),
        json!(interface.exports),
        json!(interface.link_error),
    ));
    for (index, _) in interface.exports.iter().enumerate() {
        source.push_str(&format!("$export_{index} = $exports[{index}];\n"));
    }
    Ok(source)
}

/// The facade of a `.wasm` entry file, whose record is its path.
pub(crate) fn entry_facade(path: &std::path::Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    facade(&path.to_string_lossy(), path, &bytes)
}

/// What the instantiations share, built on first use over the startup intrinsics:
/// `[instances, Module, Instance, LinkError, WeakMap.prototype.set]`, where `instances` maps
/// each facade's namespace to its instance's exports (Node's `wasmInstances`).
const BINDING: &str = "\0binding:esm-wasm";
const BINDING_MEMBERS: usize = 5;

/// Whether the experimental warning was emitted (Node's `emitExperimentalWarning` warns
/// once per feature).
const WARNED: &str = "\0binding:esm-wasm-warned";

fn get(context: &mut NativeContext<'_, NodeHost>, object: RootId, key: &str) -> Result<RootId, RootedError> {
    let key = context.string_rooted(key);
    context.get_property_rooted(object, key)
}

fn set(context: &mut NativeContext<'_, NodeHost>, object: RootId, key: &str, value: RootId) -> Result<(), RootedError> {
    let key = context.string_rooted(key);
    context.set_property_rooted(object, key, value, object).map(drop)
}

fn cached(context: &mut NativeContext<'_, NodeHost>, key: &str) -> Option<RootId> {
    context.host_mut().shared_state().borrow().builtin_cache.get(key).copied()
}

/// The entries of the facade's array argument `array` (`[[module, name], ...]` or
/// `[[name, isGlobal], ...]`).
fn pairs(context: &mut NativeContext<'_, NodeHost>, array: RootId) -> Result<Vec<(RootId, RootId)>, RootedError> {
    let length = get(context, array, "length")?;
    let length = context.to_number(length)? as usize;
    let mut pairs = Vec::with_capacity(length);
    for index in 0..length {
        let pair = get(context, array, &index.to_string())?;
        pairs.push((get(context, pair, "0")?, get(context, pair, "1")?));
    }
    Ok(pairs)
}

/// `INSTANTIATE(path, self, imports, globalImports, exports, linkError)`: Node's translator
/// callback for one facade, over the startup intrinsics' `WebAssembly`. It compiles the
/// file, prefixing a `CompileError` with the path; throws the module's `LinkError`; warns
/// once; passes an imported Wasm module's globals as their `WebAssembly.Global`;
/// instantiates; records the instance; and returns the exports' values, a global's as its
/// value (`undefined` for one without, `v128`).
pub(crate) fn instantiate(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let argument = |index: usize| args.get(index).copied().unwrap_or(undefined);
    let (path, own_namespace, imports) = (argument(0), argument(1), argument(2));
    let (global_imports, exports, link_error) = (argument(3), argument(4), argument(5));
    let binding = commonjs::cached_native_binding(context, BINDING, |context| {
        // Loading the module graph touched code that evaluating it does not run.
        crate::host::resident_pages::release_read_only_pages();
        // The warning comes after the module compiles, which releases the compiler's pages:
        // its functions compile now.
        crate::modules::process_warning_shared_vm::prepare(context)?;
        context.with_startup_intrinsics(|context| {
            let global = context.global_root()?;
            let wasm = get(context, global, "WebAssembly")?;
            let weak_map = get(context, global, "WeakMap")?;
            let instances = context.construct_rooted(weak_map, weak_map, &[])?;
            let prototype = get(context, weak_map, "prototype")?;
            let members = [instances, get(context, wasm, "Module")?, get(context, wasm, "Instance")?, get(context, wasm, "LinkError")?, get(context, prototype, "set")?];
            context.array_rooted(&members)
        })
    })?;
    let mut members = Vec::with_capacity(BINDING_MEMBERS);
    for index in 0..BINDING_MEMBERS {
        members.push(get(context, binding, &index.to_string())?);
    }
    let [instances, module, instance, link_error_class, set_instance] =
        members.try_into().map_err(|_| RootedError::host("Wasm module binding is incomplete"))?;
    let path_text = context.to_string(path)?;
    let bytes = std::fs::read(&path_text).map_err(|error| RootedError::host(format!("{path_text}: {error}")))?;
    let bytes = context.uint8_array_from_rooted(bytes)?;
    let compiled = match context.construct_rooted(module, module, &[bytes]) {
        Ok(compiled) => compiled,
        Err(RootedError { exception: Some(error), .. }) => {
            let message = get(context, error, "message")?;
            let message = context.to_string(message)?;
            let message = context.string_rooted(&format!("{path_text}: {message}"));
            set(context, error, "message", message)?;
            return Err(context.throw(error));
        }
        Err(error) => return Err(error),
    };
    if context.truthy_rooted(link_error)? {
        let error = context.construct_rooted(link_error_class, link_error_class, &[link_error])?;
        return Err(context.throw(error));
    }
    if cached(context, WARNED).is_none() {
        let warned = context.boolean(true);
        commonjs::cached_native_binding(context, WARNED, |_| Ok(warned))?;
        let process = context
            .host_mut()
            .shared_state()
            .borrow()
            .process_module
            .ok_or_else(|| RootedError::host("process is not initialized"))?;
        let emit_warning = get(context, process, "emitWarning")?;
        let message = context.string_rooted(&format!("{FEATURE} is an experimental feature and might change at any time"));
        let kind = context.string_rooted(EXPERIMENTAL_WARNING);
        context.call_rooted(emit_warning, process, &[message, kind])?;
    }
    let global_imports = pairs(context, global_imports)?;
    for module in context.own_keys_rooted(imports)? {
        let namespace = context.get_property_rooted(imports, module)?;
        let instance = context.weak_map_get_rooted(instances, namespace)?;
        if context.rooted_value(instance).is_some_and(|value| value.is_undefined()) {
            continue;
        }
        // `ObjectAssign({ __proto__: null }, namespace)`, with the instance's globals.
        let wrapped = context.null_object_rooted()?;
        for key in context.enumerable_own_keys_rooted(namespace, true, true)? {
            let value = context.get_property_rooted(namespace, key)?;
            context.set_property_rooted(wrapped, key, value, wrapped)?;
        }
        for &(from, name) in &global_imports {
            if context.strict_equal_rooted(from, module)? {
                let value = context.get_property_rooted(instance, name)?;
                context.set_property_rooted(wrapped, name, value, wrapped)?;
            }
        }
        context.set_property_rooted(imports, module, wrapped, imports)?;
    }
    let created = context.construct_rooted(instance, instance, &[compiled, imports])?;
    let instance_exports = get(context, created, "exports")?;
    context.call_rooted(set_instance, instances, &[own_namespace, instance_exports])?;
    let mut values = Vec::new();
    for (name, is_global) in pairs(context, exports)? {
        let value = context.get_property_rooted(instance_exports, name)?;
        let value = if context.truthy_rooted(is_global)? {
            let key = context.string_rooted("value");
            context.get_property_rooted(value, key).unwrap_or(undefined)
        } else {
            value
        };
        values.push(value);
    }
    context.array_rooted(&values)
}
