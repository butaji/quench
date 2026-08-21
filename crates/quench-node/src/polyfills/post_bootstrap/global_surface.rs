//! Polyfill: `global-surface`

pub const JS: &str = quench_js_check::checked_js!(r#"for (const name of Object.keys(globalThis)) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
  if (descriptor?.configurable) {
    Object.defineProperty(globalThis, name, { enumerable: false });
  }
}
"#);
