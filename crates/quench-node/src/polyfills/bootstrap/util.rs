//! Polyfill: `util`

pub const JS: &str = r#"const __quenchOriginalRequireWithSharedUtil = globalThis.require;
const __quenchSharedUtil = __quenchOriginalRequireWithSharedUtil("util");
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  if (name === "util" || name === "sys") return __quenchSharedUtil;
  return __quenchOriginalRequireWithSharedUtil(specifier);
};
"#;
