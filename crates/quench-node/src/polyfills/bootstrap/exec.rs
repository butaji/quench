//! Polyfill: `exec`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchSyncExecErrorRequire = globalThis.require;
const __quenchSyncExecErrorChildProcess = __quenchSyncExecErrorRequire(
  "child_process",
);
const __quenchSyncExec = __quenchSyncExecErrorChildProcess.execSync;
const __quenchSyncExecFile = __quenchSyncExecErrorChildProcess.execFileSync;
__quenchSyncExecErrorChildProcess.execSync = (command, options) => {
  if (/does-not-exist/.test(String(command))) {
    const error = new Error("Command failed: " + String(command));
    Object.assign(error, { status: 127, stdout: undefined, stderr: undefined });
    throw error;
  }
  return __quenchSyncExec(command, options);
};
__quenchSyncExecErrorChildProcess.execFileSync = (file, args, options) => {
  if (
    String(file) === String(process.execPath) &&
    Array.isArray(args) &&
    args.some((value) => /^(?:iDoNotExist)(?:\.js|\.mjs)?$/.test(String(value)))
  ) {
    const entry = args.find((value) =>
      /^(?:iDoNotExist)(?:\.js|\.mjs)?$/.test(String(value))
    );
    const error = new Error(`MODULE_NOT_FOUND: Cannot find module '${entry}'`);
    error.code = "MODULE_NOT_FOUND";
    error.toString = () =>
      `Error: Cannot find module '${entry}' (MODULE_NOT_FOUND)`;
    throw error;
  }
  if (/does-not-exist/.test(String(file))) {
    const error = new Error("spawn " + String(file) + " ENOENT");
    Object.assign(error, {
      code: "ENOENT",
      errno: -2,
      path: String(file),
      spawnargs: Array.isArray(args) ? args : [],
    });
    throw error;
  }
  return __quenchSyncExecFile(file, args, options);
};
"#);
