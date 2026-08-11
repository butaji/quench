//! Polyfill: `reporters`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithTestReporters = globalThis.require;
globalThis.require = (specifier) => {
  if (
    String(specifier) === "node:test/reporters" ||
    String(specifier) === "test/reporters"
  ) {
    const error = new Error(`No such built-in module: ${specifier}`);
    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";
    throw error;
  }
  return __quenchOriginalRequireWithTestReporters(specifier);
};
"#);
