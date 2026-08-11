//! Polyfill: `sys`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithSys = globalThis.require;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "sys") {
    return __quenchOriginalRequireWithSys("util");
  }
  return __quenchOriginalRequireWithSys(specifier);
};
"#);
