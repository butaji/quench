globalThis.__quench_argv ||= [];
if (globalThis.process) {
  globalThis.process.stdout ||= {};
  globalThis.process.stdout.write ||= (chunk) => {
    globalThis.__quench_console_write(String(chunk));
    return true;
  };
}
if (globalThis.require) {
  const __quenchProcessModule = globalThis.require("process");
  __quenchProcessModule.stdout ||= {};
  __quenchProcessModule.stdout.write ||= (chunk) => {
    globalThis.__quench_console_write(String(chunk));
    return true;
  };
}
