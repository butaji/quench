//! Polyfill: `process-surface-15`

pub const JS: &str = quench_js_check::checked_js!(r#"if (globalThis.process) {
  for (const stream of [globalThis.process.stdout, globalThis.process.stderr]) {
    if (!stream) continue;
    stream.writableHighWaterMark = 65536;
    if (stream.constructor.name !== "Socket") {
      Object.defineProperty(stream, "constructor", {
        value: function Socket() {},
        configurable: true
      });
    }
  }
}
"#);
