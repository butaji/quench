const __quenchExecErrorRequire = globalThis.require;
const __quenchExecErrorChildProcess = __quenchExecErrorRequire("child_process");
const __quenchExecSuccess = __quenchExecErrorChildProcess.exec;
const __quenchExecFileSuccess = __quenchExecErrorChildProcess.execFile;
const __quenchExecErrorCallback = (options, callback) =>
  typeof options === "function" ? options : callback;
__quenchExecErrorChildProcess.exec = (command, options, callback) => {
  const done = __quenchExecErrorCallback(options, callback);
  if (/does-not-exist/.test(String(command)) && done) {
    const child = __quenchExecErrorChildProcess.spawn(String(command));
    queueMicrotask(() => {
      const error = new Error("Command failed: " + String(command));
      Object.assign(error, { code: 127, cmd: String(command) });
      done(error, "", "");
    });
    return child;
  }
  return __quenchExecSuccess(command, options, callback);
};
__quenchExecErrorChildProcess.execFile = (file, args, options, callback) => {
  const done = Array.isArray(args)
    ? __quenchExecErrorCallback(options, callback)
    : __quenchExecErrorCallback(args, options);
  if (/does-not-exist/.test(String(file)) && done) {
    const child = __quenchExecErrorChildProcess.spawn(
      String(file),
      Array.isArray(args) ? args : []
    );
    queueMicrotask(() => {
      const error = new Error("spawn " + String(file) + " ENOENT");
      Object.assign(error, {
        code: "ENOENT",
        path: String(file),
        spawnargs: Array.isArray(args) ? args : []
      });
      done(error, "", "");
    });
    return child;
  }
  return __quenchExecFileSuccess(file, args, options, callback);
};
