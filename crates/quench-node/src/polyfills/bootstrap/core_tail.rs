//! Polyfill: `core-tail`

pub const JS: &str = r#"globalThis.__quench_require_part_00 = (name, specifier) => {
  const normalizedName = String(name);
  const base = globalThis.__quench_require_part_00_base(
    normalizedName.startsWith("node:")
      ? normalizedName.slice(5)
      : normalizedName,
  );
  if (base !== undefined) return base;
  if (name === "http") return globalThis.__nodeHttp || __quenchHttpModule;
  if (name === "child_process") {
    return globalThis.__nodeRequireChildProcess || __quenchChildProcessModule();
  }
};
"#;
