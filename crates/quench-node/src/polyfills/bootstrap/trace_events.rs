//! Polyfill: `trace-events`

pub const JS: &str = r#"const __quenchOriginalRequireWithTraceEvents = globalThis.require;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "trace_events") {
    const error = new Error(`No such built-in module: ${specifier}`);
    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";
    throw error;
  }
  return __quenchOriginalRequireWithTraceEvents(specifier);
};
"#;
