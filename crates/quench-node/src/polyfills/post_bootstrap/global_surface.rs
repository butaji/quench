//! Polyfill: `global-surface`

pub const JS: &str = r#"for (const name of Object.keys(globalThis)) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
  if (descriptor?.configurable) {
    Object.defineProperty(globalThis, name, { enumerable: false });
  }
}
"#;
