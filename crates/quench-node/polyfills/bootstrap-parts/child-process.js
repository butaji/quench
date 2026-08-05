const __quenchChildProcessRequire = globalThis.require;
const __quenchChildProcess = __quenchChildProcessRequire("child_process");
const __quenchExecCallback = (options, callback) =>
  typeof options === "function" ? options : callback;
const __quenchExec = (command, options, callback) => {
  const done = __quenchExecCallback(options, callback);
  const child = __quenchChildProcess.spawn(command);
  if (done) queueMicrotask(() => done(null, "", ""));
  return child;
};
__quenchChildProcess.exec = __quenchExec;
__quenchChildProcess.execFile = (file, args, options, callback) => {
  const values = Array.isArray(args) ? args : [];
  const done = Array.isArray(args)
    ? __quenchExecCallback(options, callback)
    : __quenchExecCallback(args, options);
  const child = __quenchChildProcess.spawn(file, values);
  const output = String(file).endsWith("echo") ? `${values.join(" ")}\n` : "";
  if (done) queueMicrotask(() => done(null, output, ""));
  return child;
};
__quenchChildProcess.execSync = () => NodeBuffer.from("");
__quenchChildProcess.execFileSync = (file, args = [], options) => {
  const output = String(file).endsWith("echo") ? `${args.join(" ")}\n` : "";
  return options?.encoding ? output : NodeBuffer.from(output);
};
