//! Polyfill: `console`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithConsole = globalThis.require;
const __quenchConsoleReceiver = (receiver) =>
  receiver !== null &&
  (typeof receiver === "object" || typeof receiver === "function")
    ? receiver
    : globalThis.console;
class __quenchConsole {
  constructor(
    stdout = globalThis.process?.stdout,
    stderr = globalThis.process?.stderr,
  ) {
    this._stdout = stdout;
    this._stderr = stderr;
  }
  log(...args) {
    const output = __quenchConsoleReceiver(this)._stdout || globalThis.process?.stdout;
    if (output && typeof output.write === "function") {
      const format = globalThis.__nodeUtil && globalThis.__nodeUtil.format;
      const text = typeof format === "function" ? format(...args) : args.join(" ");
      output.write(`${text}\n`);
    }
  }
  info(...args) {
    return __quenchConsole.prototype.log.call(__quenchConsoleReceiver(this), ...args);
  }
  warn(...args) {
    const output = __quenchConsoleReceiver(this)._stderr || globalThis.process?.stderr;
    if (output && typeof output.write === "function") {
      const format = globalThis.__nodeUtil && globalThis.__nodeUtil.format;
      const text = typeof format === "function" ? format(...args) : args.join(" ");
      output.write(`${text}\n`);
    }
  }
  error(...args) {
    return __quenchConsole.prototype.warn.call(__quenchConsoleReceiver(this), ...args);
  }
  dir(value) {
    return __quenchConsole.prototype.log.call(__quenchConsoleReceiver(this), value);
  }
  table(value) {
    return __quenchConsole.prototype.log.call(__quenchConsoleReceiver(this),
      Array.isArray(value)
        ? value.map((item) => JSON.stringify(item)).join("\n")
        : JSON.stringify(value),
    );
  }
  trace(...args) {
    return __quenchConsole.prototype.warn.call(__quenchConsoleReceiver(this), ...args);
  }
  assert(condition, ...args) {
    if (!condition) __quenchConsole.prototype.warn.call(__quenchConsoleReceiver(this), ...args);
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
  error: (...args) => globalThis.console.error(...args),
  dir: (...args) => globalThis.console.dir?.(...args),
  createTask: (name) => ({
    name: String(name || ""),
    run: (callback, ...args) => callback(...args),
  }),
};
for (const name of ["log", "info", "warn", "error"]) {
  const original = globalThis.console?.[name];
  if (typeof original !== "function") continue;
  globalThis.console[name] = (...args) =>
    original.call(
      globalThis.console,
      globalThis.__nodeUtil?.format?.(...args) ?? args.join(" "),
    );
}
for (const name of ["time", "timeEnd", "timeLog"]) {
  const original = globalThis.console?.[name];
  if (typeof original !== "function") continue;
  globalThis.console[name] = (label, ...args) => {
    if (typeof label === "symbol") throw new TypeError("Invalid console label");
    const text = String(label);
    globalThis.console._times ||= new Map();
    if (name === "time" && !globalThis.console._times.has(text)) {
      globalThis.console._times.set(text, Date.now());
    }
    if (name === "timeEnd") globalThis.console._times.delete(text);
    const safeLabel = ["__proto__", "constructor", "hasOwnProperty"].includes(
        text,
      )
      ? `__quench_${text}`
      : label;
    return original.call(globalThis.console, safeLabel, ...args);
  };
}
globalThis.console.dirxml ||= (...args) => globalThis.console.log(...args);
globalThis.console.trace ||= (...args) => globalThis.console.error(...args);
globalThis.console.assert = (condition, ...args) => {
  if (condition) return;
  const originalDetail = args[0] || "";
  let used = 1;
  const text = String(originalDetail);
  const placeholder = text.indexOf("%s");
  let detail = placeholder >= 0
    ? `${text.slice(0, placeholder)}${String(args[used++])}${
      text.slice(
        placeholder + 2,
      )
    }`
    : text;
  if (used < args.length) detail += ` ${args.slice(used).join(" ")}`;
  const message = detail ? `Assertion failed: ${detail}` : "Assertion failed";
  globalThis.process?.stderr?.write?.(`${message}\n`);
};
__quenchConsole.prototype.dirxml ||= function (...args) {
  return __quenchConsole.prototype.log.call(__quenchConsoleReceiver(this), ...args);
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "console") {
    return __quenchConsoleModule;
  }
  return __quenchOriginalRequireWithConsole(specifier);
};
"#);
