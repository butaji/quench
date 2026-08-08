const __quenchSpawnSyncErrorRequire = globalThis.require;
const __quenchSpawnSyncErrorChildProcess =
  __quenchSpawnSyncErrorRequire("child_process");
const __quenchOriginalSpawnSync = __quenchSpawnSyncErrorChildProcess.spawnSync;
__quenchSpawnSyncErrorChildProcess.spawnSync = (
  command,
  args = [],
  options
) => {
  if (/not_a_real_command|does-not-exist/.test(String(command))) {
    const error = new Error("spawn " + String(command) + " ENOENT");
    Object.assign(error, {
      code: "ENOENT",
      errno: -2,
      syscall: `spawnSync ${String(command)}`,
      path: String(command),
      spawnargs: Array.isArray(args) ? args : []
    });
    return {
      pid: 0,
      status: null,
      signal: null,
      output: [],
      stdout: undefined,
      stderr: undefined,
      error
    };
  }
  return __quenchOriginalSpawnSync(command, args, options);
};
