const __quenchExecEncodingRequire = globalThis.require;
const __quenchExecEncodingChildProcess = __quenchExecEncodingRequire(
  "child_process",
);
const __quenchExecEncodingOriginal = __quenchExecEncodingChildProcess.exec;
__quenchExecEncodingChildProcess.exec = (command, options, callback) => {
  const settings = options && typeof options === "object" ? options : {};
  const done = typeof options === "function" ? options : callback;
  if (!done) return __quenchExecEncodingOriginal(command, options, callback);
  return __quenchExecEncodingOriginal(
    command,
    options,
    (error, stdout, stderr) => {
      if (
        Object.hasOwn(settings, "encoding") &&
        settings.encoding !== "utf8" &&
        settings.encoding !== "ascii" &&
        settings.encoding !== "latin1"
      ) {
        stdout = Buffer.from(stdout || "");
        stderr = Buffer.from(stderr || "");
      }
      done(error, stdout, stderr);
    },
  );
};
