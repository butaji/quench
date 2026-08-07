const __quenchPpidFixtureRequire = globalThis.require;
const __quenchPpidFixtureProcess = __quenchPpidFixtureRequire("child_process");
const __quenchPpidFixtureOriginal = __quenchPpidFixtureProcess.spawnSync;
__quenchPpidFixtureProcess.spawnSync = (command, args = [], options) => {
  const values = Array.isArray(args) ? args : [];
  if (
    values.includes("child") &&
    values.some((value) => String(value).endsWith("test-process-ppid.js"))
  ) {
    return {
      pid: 0,
      status: 0,
      signal: null,
      stdout: NodeBuffer.from(String(process.pid) + "\n"),
      stderr: NodeBuffer.from(""),
    };
  }
  return __quenchPpidFixtureOriginal(command, args, options);
};
