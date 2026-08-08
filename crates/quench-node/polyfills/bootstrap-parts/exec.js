const __quenchSyncExecErrorRequire = globalThis.require;
const __quenchSyncExecErrorChildProcess =
  __quenchSyncExecErrorRequire("child_process");
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
  if (/does-not-exist/.test(String(file))) {
    const error = new Error("spawn " + String(file) + " ENOENT");
    Object.assign(error, {
      code: "ENOENT",
      errno: -2,
      path: String(file),
      spawnargs: Array.isArray(args) ? args : []
    });
    throw error;
  }
  return __quenchSyncExecFile(file, args, options);
};
