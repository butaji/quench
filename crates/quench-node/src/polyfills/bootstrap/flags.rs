//! Polyfill: `flags`

pub const JS: &str = r#"globalThis.__quench_argv ||= [];
const __quenchProcessWrite = (chunk) => {
  globalThis.__quench_console_write(String(chunk));
  return true;
};
if (globalThis.process) {
  globalThis.process.stdout ||= {};
  globalThis.process.stdout.write ||= __quenchProcessWrite;
}
if (globalThis.require) {
  const __quenchProcessModule = globalThis.require("process");
  __quenchProcessModule.stdout ||= {};
  __quenchProcessModule.stdout.write ||= __quenchProcessWrite;
}
"#;
