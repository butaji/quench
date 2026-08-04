const __quenchOriginalRequireWithConsole = globalThis.require;
class __quenchConsole {
  constructor(
    stdout = globalThis.process?.stdout,
    stderr = globalThis.process?.stderr
  ) {
    this._stdout = stdout;
    this._stderr = stderr;
  }
  log(...args) {
    this._stdout?.write?.(`${args.join(" ")}\n`);
  }
  info(...args) {
    this.log(...args);
  }
  warn(...args) {
    this._stderr?.write?.(`${args.join(" ")}\n`);
  }
  error(...args) {
    this.warn(...args);
  }
  table(value) {
    this.log(
      Array.isArray(value)
        ? value.map((item) => JSON.stringify(item)).join("\n")
        : JSON.stringify(value)
    );
  }
  trace(...args) {
    this.error(...args);
  }
  assert(condition, ...args) {
    if (!condition) this.error(...args);
  }
  group() {}
  groupCollapsed() {}
  groupEnd() {}
}
const __quenchConsoleModule = {
  Console: __quenchConsole,
  _stdout: globalThis.process?.stdout,
  _stderr: globalThis.process?.stderr,
  log: (...args) => globalThis.console.log(...args),
  info: (...args) => globalThis.console.info(...args),
  warn: (...args) => globalThis.console.warn(...args),
  error: (...args) => globalThis.console.error(...args)
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "console")
    return __quenchConsoleModule;
  return __quenchOriginalRequireWithConsole(specifier);
};
