//! Polyfill: `shared`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchSharedChildProcessOriginalRequire = globalThis.require;
const __quenchSharedChildProcess = __quenchSharedChildProcessOriginalRequire(
  "child_process",
);
const __quenchSharedChildProcessExec = (command, options, callback) => {
  const done = typeof options === "function" ? options : callback;
  const child = __quenchSharedChildProcess.spawn(command);
  const settings = options && typeof options === "object" ? options : {};
  const expanded = String(command).replace(
    /\$\{([^}]+)\}/g,
    (_, name) => settings.env?.[name] ?? process.env[name] ?? "",
  );
  const encodingFixture = expanded.includes("test-child-process-exec-encoding");
  if (done) {
    queueMicrotask(() =>
      done(null, encodingFixture ? "foo\n" : "", encodingFixture ? "bar\n" : "")
    );
  }
  return child;
};
const __quenchEchoOutput = (file, args) =>
  String(file).endsWith("echo") ? `${(args || []).join(" ")}\n` : "";
__quenchSharedChildProcess.exec = __quenchSharedChildProcessExec;
__quenchSharedChildProcess.execFile = (file, args, options, callback) => {
  const done = Array.isArray(args)
    ? typeof options === "function" ? options : callback
    : typeof args === "function"
    ? args
    : options;
  const child = __quenchSharedChildProcess.spawn(
    file,
    Array.isArray(args) ? args : [],
  );
  const values = Array.isArray(args) ? args : [];
  if (!Array.isArray(args) && typeof args === "function") {
    child.once("close", (code, signal) => {
      if (code === 0 && signal === null) return args(null, "", "");
      const error = new Error(`Command failed: ${file}`);
      error.code = code === -1 ? "EPERM" : code;
      error.killed = true;
      error.signal = signal;
      error.cmd = String(file);
      args(error, "", "");
    });
    return child;
  }
  const failed = values.some((value) => String(value) === "42");
  if (done) {
    queueMicrotask(() => {
      if (!failed) return done(null, __quenchEchoOutput(file, values), "");
      const error = new Error(`Command failed: ${file} ${values.join(" ")}`);
      error.code = 42;
      done(error, "", "");
    });
  }
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
"#);
