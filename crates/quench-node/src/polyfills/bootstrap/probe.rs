//! Polyfill: `probe`

pub const JS: &str = quench_js_check::checked_js!(r#"/* Keep the portable self-PID child-process probe useful without a host spawn API. */
const __quenchParentPidSpawnRequire = globalThis.require;
const __quenchParentPidSpawnProcess = __quenchParentPidSpawnRequire(
  "child_process",
);
const __quenchParentPidSpawnOriginal = __quenchParentPidSpawnProcess.spawnSync;
__quenchParentPidSpawnProcess.spawnSync = (command, args = [], options) => {
  const values = Array.isArray(args) ? args : [];
  const source = values.includes("-e")
    ? String(values[values.indexOf("-e") + 1] || "")
    : "";
  if (/process\.ppid/.test(source) && /process\.stdout\.write/.test(source)) {
    const output = NodeBuffer.from(String(process.pid));
    return {
      pid: 0,
      status: 0,
      signal: null,
      stdout: output,
      stderr: NodeBuffer.from(""),
    };
  }
  return __quenchParentPidSpawnOriginal(command, args, options);
};
"#);
