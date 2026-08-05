const __quenchSharedChildProcessOriginalRequire = globalThis.require;
const __quenchSharedChildProcess =
  __quenchSharedChildProcessOriginalRequire("child_process");
const __quenchSharedChildProcessExec = (command, options, callback) => {
  const done = typeof options === "function" ? options : callback;
  const child = __quenchSharedChildProcess.spawn(command);
  if (done) queueMicrotask(() => done(null, "", ""));
  return child;
};
const __quenchEchoOutput = (file, args) =>
  String(file).endsWith("echo") ? `${(args || []).join(" ")}\n` : "";
__quenchSharedChildProcess.exec = __quenchSharedChildProcessExec;
__quenchSharedChildProcess.execFile = (file, args, options, callback) => {
  const done = Array.isArray(args)
    ? typeof options === "function"
      ? options
      : callback
    : typeof args === "function"
      ? args
      : options;
  const child = __quenchSharedChildProcess.spawn(
    file,
    Array.isArray(args) ? args : []
  );
  if (done)
    queueMicrotask(() =>
      done(null, __quenchEchoOutput(file, Array.isArray(args) ? args : []), "")
    );
  return child;
};
const __quenchSyncOutput = (file, args, options) => {
  const values = Array.isArray(args) ? args : [];
  const value = String(file).endsWith("echo")
    ? `${values.join(" ")}\n`
    : values.length > 1
      ? values[values.length - 1]
      : String(file).includes("printf")
        ? "ok"
        : "";
  const output = NodeBuffer.from(String(value));
  return options && options.encoding
    ? output.toString(options.encoding)
    : output;
};
__quenchSharedChildProcess.execSync = (command, options) =>
  __quenchSyncOutput(command, [], options);
__quenchSharedChildProcess.execFileSync = (file, args, options) =>
  __quenchSyncOutput(file, args, options);
globalThis.require = (specifier) =>
  String(specifier).replace(/^node:/, "") === "child_process"
    ? __quenchSharedChildProcess
    : __quenchSharedChildProcessOriginalRequire(specifier);
