//! Polyfill: `consumers`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithConsumers = globalThis.require;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "stream/consumers") {
    return globalThis.__quenchNativeRequire?.("stream/consumers");
  }
  return __quenchOriginalRequireWithConsumers(specifier);
};
"#);
