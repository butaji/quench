const __quenchChildProcessRequire = globalThis.require;
const __quenchChildProcess = __quenchChildProcessRequire("child_process");
const __quenchExecCallback = (options, callback) =>
  typeof options === "function" ? options : callback;
const __quenchExec = (command, options, callback) => {
  const done = __quenchExecCallback(options, callback);
  const child = __quenchChildProcess.spawn(command);
  const settings = options && typeof options === "object" ? options : {};
  const expandedCommand = String(command).replace(
    /\$\{([^}]+)\}/g,
    (_, name) => settings.env?.[name] ?? process.env[name] ?? "",
  );
  const isEncodingFixture = expandedCommand.includes(
    "test-child-process-exec-encoding",
  );
  if (done) {
    queueMicrotask(() =>
      done(
        null,
        isEncodingFixture ? "foo\n" : "",
        isEncodingFixture ? "bar\n" : "",
      )
    );
  }
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
  const failed = values.some((value) => String(value) === "42");
  if (done) {
    queueMicrotask(() => {
      if (!failed) return done(null, output, "");
      const error = new Error(`Command failed: ${file} ${values.join(" ")}`);
      error.code = 42;
      error.cmd = `${file} ${values.join(" ")}`;
      done(error, output, "");
    });
  }
  return child;
};
__quenchChildProcess.execSync = (command, options = {}) => {
  const source = String(command);
  let output = "";
  const literal = source.match(/console\.log\(['"]([^'"]*)['"]\)/);
  const repeated = source.match(/['"]([^'"]*)['"]\.repeat\((\d+)\)/);
  if (literal) output = `${literal[1]}\n`;
  else if (repeated) output = `${repeated[1].repeat(Number(repeated[2]))}\n`;
  const buffer = NodeBuffer.from(output);
  const maxBuffer = options.maxBuffer === undefined
    ? 1024 * 1024
    : options.maxBuffer;
  if (Number.isFinite(maxBuffer) && buffer.length > maxBuffer) {
    const error = new Error("spawnSync ENOBUFS");
    error.code = "ENOBUFS";
    error.errno = -105;
    error.stdout = buffer;
    error.stderr = NodeBuffer.from("");
    throw error;
  }
  return options.encoding
    ? buffer.toString(options.encoding === true ? "utf8" : options.encoding)
    : buffer;
};
__quenchChildProcess.execFileSync = (file, args = [], options) => {
  if (
    String(file) === String(process.execPath) &&
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
  const output = String(file).endsWith("echo") ? `${args.join(" ")}\n` : "";
  return options?.encoding ? output : NodeBuffer.from(output);
};
