//! Polyfill: `metrics`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchProcessStart = Date.now();
const __quenchOriginalRequireWithProcessMetrics = globalThis.require;
if (globalThis.process) {
  globalThis.process.uptime ||= () =>
    (Date.now() - __quenchProcessStart) / 1000;
  globalThis.process.memoryUsage ||= () => ({
    rss: 0,
    heapTotal: 0,
    heapUsed: 0,
    external: 0,
    arrayBuffers: 0,
  });
  globalThis.process.cpuUsage ||= () => ({ user: 0, system: 0 });
}
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "process") {
    return globalThis.process;
  }
  return __quenchOriginalRequireWithProcessMetrics(specifier);
};
"#);
