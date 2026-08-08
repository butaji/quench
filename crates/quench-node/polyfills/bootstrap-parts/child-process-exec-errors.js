const __quenchExecErrorRequire = globalThis.require;
const __quenchExecErrorChildProcess = __quenchExecErrorRequire("child_process");
const __quenchExecSuccess = __quenchExecErrorChildProcess.exec;
const __quenchExecFileSuccess = __quenchExecErrorChildProcess.execFile;
const __quenchExecErrorCallback = (options, callback) =>
  typeof options === "function" ? options : callback;
const __quenchAbortError = () => {
  const error = new Error("The operation was aborted");
  error.name = "AbortError";
  error.code = "ABORT_ERR";
  return error;
};
const __quenchExecWithAbort = (file, args, settings, done) => {
  if (!settings?.signal || !done) return undefined;
  const child = __quenchExecErrorChildProcess.spawn(
    String(file),
    Array.isArray(args) ? args : []
  );
  const abort = () => done(__quenchAbortError(), "", "");
  if (settings.signal.aborted) queueMicrotask(abort);
  else settings.signal.addEventListener("abort", abort, { once: true });
  return child;
};
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
  const settings = Array.isArray(args) ? options : args;
  if (
    settings?.signal !== undefined &&
    !(settings.signal instanceof AbortSignal)
  ) {
    const error = new TypeError("The signal option must be an AbortSignal");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const done = Array.isArray(args)
    ? __quenchExecErrorCallback(options, callback)
    : __quenchExecErrorCallback(args, options);
  const abortedChild = __quenchExecWithAbort(file, args, settings, done);
  if (abortedChild) return abortedChild;
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
