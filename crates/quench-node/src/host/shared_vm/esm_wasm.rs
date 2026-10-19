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

use super::commonjs::cached_native_binding;
use crate::host::node_host::NodeHost;

/// The hidden global the facades call.
pub(crate) const INSTANTIATE: &str = "\0quench:wasmInstantiate";

/// Node's `emitExperimentalWarning` feature name for importing Wasm module instances.
const FEATURE: &str = "Importing WebAssembly module instances";

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

/// The instantiation the facades share, built once over the startup intrinsics: Node's
/// translator callback with the module's bytes read by `read`.
const INSTANTIATE_FACTORY: &str = r#"(read, feature) => {
  'use strict';
  const { Module, Instance, LinkError } = WebAssembly;
  const { assign } = Object;
  const instances = new WeakMap();
  const getInstance = WeakMap.prototype.get;
  const setInstance = WeakMap.prototype.set;
  const call = Function.prototype.call.bind(Function.prototype.call);
  // Node's `emitExperimentalWarning` calls the public `process.emitWarning` when it warns.
  const host = process;
  let warned = false;
  return function instantiate(path, self, imports, globalImports, exports, linkError) {
    let compiled;
    try {
      compiled = new Module(read(path));
    } catch (error) {
      error.message = path + ': ' + error.message;
      throw error;
    }
    if (linkError !== null) throw new LinkError(linkError);
    if (!warned) {
      warned = true;
      host.emitWarning(`${feature} is an experimental feature and might change at any time`, 'ExperimentalWarning');
    }
    for (const module in imports) {
      const instance = call(getInstance, instances, imports[module]);
      if (instance) {
        const wrapped = assign({ __proto__: null }, imports[module]);
        for (const [from, name] of globalImports) {
          if (from === module) wrapped[name] = instance[name];
        }
        imports[module] = wrapped;
      }
    }
    const instance = new Instance(compiled, imports).exports;
    call(setInstance, instances, self, instance);
    const values = [];
    for (const [name, global] of exports) {
      let value = instance[name];
      if (global) {
        try {
          value = value.value;
        } catch {
          value = undefined;
        }
      }
      values.push(value);
    }
    return values;
  };
}"#;

/// `INSTANTIATE`: builds the shared instantiation on first use and runs it for one facade.
pub(crate) fn instantiate(
    context: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let instantiate = cached_native_binding(context, "\0binding:esm-wasm", |context| {
        context.with_startup_intrinsics(|context| {
            let factory = context.evaluate_script_rooted(INSTANTIATE_FACTORY, "node:internal/modules/esm/wasm")?;
            let read = context.host_function(crate::host::shared_vm::operation("esmWasmRead"))?;
            let feature = context.string_rooted(FEATURE);
            let undefined = context.undefined();
            context.call_rooted(factory, undefined, &[read, feature])
        })
    })?;
    context.call_rooted(instantiate, this, args)
}

/// The bytes of the Wasm file a facade names, as a `Uint8Array`.
pub(crate) fn read(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let path = args.first().copied().unwrap_or(undefined);
    let path = context.to_string(path)?;
    let bytes = std::fs::read(&path).map_err(|error| RootedError::host(format!("{path}: {error}")))?;
    context.uint8_array_from_rooted(bytes)
}
