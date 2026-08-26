//! Polyfill: `ppid`

// Keep the process-parent relation available even when an embedding realm
// supplies a minimal process object instead of the standard host object.
pub const JS: &str = quench_js_check::checked_js!(r#"if (globalThis.process && typeof globalThis.process.ppid !== "number") {
  const parent = globalThis.process.env?.QUENCH_PARENT_PID;
  Object.defineProperty(globalThis.process, "ppid", {
    configurable: true,
    enumerable: true,
    value: Number(parent || 0),
  });
}
"#);
