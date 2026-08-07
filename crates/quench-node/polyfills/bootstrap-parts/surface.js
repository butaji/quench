const __quenchChildProcessSurfaceRequire = globalThis.require;
const __quenchChildProcessSurface = __quenchChildProcessSurfaceRequire(
  "child_process",
);
const __quenchOriginalSpawn = __quenchChildProcessSurface.spawn;
const __quenchStream = () => {
  const stream = new globalThis.__nodeEventEmitter();
  stream.write = () => true;
  stream.end = () => stream;
  return stream;
};
__quenchChildProcessSurface.spawn = (...args) => {
  const child = __quenchOriginalSpawn(...args);
  child.connected = false;
  child.killed = false;
  child.exitCode = undefined;
  child.signalCode = null;
  const spawnOptions = Array.isArray(args[1]) ? args[2] : args[1];
  child.spawnargs = spawnOptions?.shell
    ? [
      "-c",
      `${String(args[0])}${
        Array.isArray(args[1]) && args[1].length ? ` ${args[1].join(" ")}` : ""
      }`,
    ]
    : args[1] || [];
  child.spawnfile = spawnOptions?.shell
    ? (process.platform === "win32" ? "cmd.exe" : "/bin/sh")
    : args[0];
  child.stdin ||= __quenchStream();
  child.stdout ||= __quenchStream();
  child.stderr ||= __quenchStream();
  child.stdio = [child.stdin, child.stdout, child.stderr];
  child.ref = () => (args[1]?.length ? child : undefined);
  child.unref = () => child;
  const originalEmit = child.emit.bind(child);
  child.emit = (event, ...values) =>
    event === "exit" && child.killed
      ? originalEmit("exit", null, child.signalCode)
      : originalEmit(event, ...values);
  child.kill = (signal = "SIGTERM") => {
    child.killed = true;
    child.signalCode = signal;
    return true;
  };
  return child;
};
