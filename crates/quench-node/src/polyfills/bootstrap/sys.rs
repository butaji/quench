//! Polyfill: `sys`

pub const JS: &str = r#"const __quenchOriginalRequireWithSys = globalThis.require;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "sys") {
    return __quenchOriginalRequireWithSys("util");
  }
  return __quenchOriginalRequireWithSys(specifier);
};
"#;
