//! Polyfill: `wasi`

pub const JS: &str = r#"const __quenchOriginalRequireWithWasi = globalThis.require;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "wasi") {
    const error = new Error(`No such built-in module: ${specifier}`);
    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";
    throw error;
  }
  return __quenchOriginalRequireWithWasi(specifier);
};
"#;
