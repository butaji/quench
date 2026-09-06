//! Polyfill: `report`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithProcessReport = globalThis.require;
const __quenchReportProcess = globalThis.process;
const __quenchReportLibuv = [];
__quenchReportLibuv.filter ||= () => [];
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
        pid: __quenchReportProcess && __quenchReportProcess.pid || 0,
        commandLine: __quenchReportProcess && __quenchReportProcess.argv || [],
      },
      javascriptStack: { message: "" },
      resourceUsage: {},
      libuv: __quenchReportLibuv,
      sharedObjects: [],
    }),
    writeReport: () => undefined,
  };
}
globalThis.require = (specifier) =>
  String(specifier).replace(/^node:/, "") === "process"
    ? globalThis.process
    : __quenchOriginalRequireWithProcessReport(specifier);
"#);
