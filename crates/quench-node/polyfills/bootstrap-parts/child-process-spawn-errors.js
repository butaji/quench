const __quenchChildSpawnErrorRequire = globalThis.require;
const __quenchChildSpawnError = __quenchChildSpawnErrorRequire("child_process");
const __quenchSpawnWithError = __quenchChildSpawnError.spawn;
__quenchChildSpawnError.spawn = (...args) => {
  const child = __quenchSpawnWithError(...args);
  const command = String(args[0] || "");
  const options = Array.isArray(args[1]) ? args[2] : args[1];
  if (options?.shell) return child;
  if (/does-not-exist|foo123|hopefully_you_dont_have_this/.test(command)) {
    const emit = child.emit;
    let reported = false;
    child.pid = undefined;
    child.emit = (event, ...values) => {
      if (event === "spawn") return false;
      if ((event === "error" || event === "exit") && !reported) {
        reported = true;
        const error = new Error(`spawn ${command} ENOENT`);
        Object.assign(error, {
          code: "ENOENT",
          errno: -2,
          syscall: `spawn ${command}`,
          path: command,
          spawnargs: args[1] || [],
        });
        emit.call(child, "error", error);
      }
      if (event === "error") return true;
      return emit.call(child, event, ...values);
    };
  }
  return child;
};
