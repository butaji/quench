//! Polyfill: `inspector`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithInspector = globalThis.require;
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  if (name === "inspector" || name === "inspector/promises") {
    const error = new Error(`No such built-in module: ${specifier}`);
    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";
    throw error;
  }
  return __quenchOriginalRequireWithInspector(specifier);
};
"#);
