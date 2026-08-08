const __quenchOriginalRequireWithProcessReport = globalThis.require;
if (globalThis.process && !globalThis.process.report) {
  globalThis.process.report = {
    directory: "",
    filename: "",
    signal: "SIGUSR2",
    compact: false,
    reportOnFatalError: false,
    reportOnSignal: false,
    reportOnUncaughtException: false,
    getReport: () => ({
      header: {
        event: "JavaScript API",
        pid: globalThis.process.pid,
        commandLine: globalThis.process.argv
      },
      javascriptStack: { message: "" },
      resourceUsage: {},
      libuv: [],
      sharedObjects: []
    }),
    writeReport: () => undefined
  };
}
globalThis.require = (specifier) =>
  String(specifier).replace(/^node:/, "") === "process"
    ? globalThis.process
    : __quenchOriginalRequireWithProcessReport(specifier);
