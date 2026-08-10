//! Polyfill: `sqlite`

pub const JS: &str = r#"const __quenchOriginalRequireWithSqlite = globalThis.require;
globalThis.require = (specifier) => {
  if (String(specifier) === "node:sqlite" || String(specifier) === "sqlite") {
    const error = new Error(`No such built-in module: ${specifier}`);
    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";
    throw error;
  }
  return __quenchOriginalRequireWithSqlite(specifier);
};
"#;
