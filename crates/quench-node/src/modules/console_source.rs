//! Canonical JavaScript source shared by console runtime adapters.

pub(crate) const CONSOLE_CLASS: &str = r#"(class Console {
  constructor(stdout, stderr) {
    const options = stdout && typeof stdout === "object" &&
      (stdout.stdout || stdout.stderr) ? stdout : null;
    this._stdout = options ? options.stdout : stdout;
    this._stderr = options ? options.stderr : stderr;
    this._inspectOptions = options ? options.inspectOptions : undefined;
    if (!this._stdout) this._stdout = globalThis?.process?.stdout;
    if (!this._stderr) this._stderr = globalThis?.process?.stderr;
  }
  _format(output, args) {
    const util = (typeof require === "function"
        ? require("util")
        : undefined);
    const format = util?.format;
    const formatWithOptions = util?.formatWithOptions;
    const configured = this._inspectOptions &&
      typeof this._inspectOptions.get === "function"
      ? this._inspectOptions.get(output)
      : this._inspectOptions;
    if (configured && typeof formatWithOptions === "function") {
      return formatWithOptions(configured, ...args);
    }
    return typeof format === "function" ? format(...args) : args.join(" ");
  }
  log(...args) {
    const output = this._stdout || process?.stdout;
    if (output && typeof output.write === "function") output.write(`${this._format(output, args)}\n`);
    if (!this._tickPending) {
      this._tickPending = true;
      const tick = globalThis?.process?.nextTick;
      if (typeof tick === "function") tick(() => { this._tickPending = false; });
    }
  }
  info(...args) { this.log(...args); }
  dir(...args) { this.log(...args); }
  time(label = "default") { if (typeof label === "symbol") throw new TypeError("Invalid console label"); this._times ||= new Map(); if (!this._times.has(label)) this._times.set(label, Date.now()); }
  timeEnd(label = "default") { if (typeof label === "symbol") throw new TypeError("Invalid console label"); this._times?.delete(label); }
  timeLog(label = "default", ...args) { this.log(...args); }
  warn(...args) {
    const output = this._stderr || process?.stderr;
    if (output && typeof output.write === "function") output.write(`${this._format(output, args)}\n`);
  }
  error(...args) { this.warn(...args); }
  trace(...args) { this.error(...args); }
  assert(condition, ...args) { if (!condition) this.error(...args); }
  clear() {}
  count(label = "default") { this._counts ||= new Map(); this._counts.set(label, (this._counts.get(label) || 0) + 1); }
  countReset(label = "default") { this._counts?.delete(label); }
  group() {}
  groupEnd() {}
  table(...args) { this.log(...args); }
  debug(...args) { this.log(...args); }
  dirxml(...args) { this.log(...args); }
  groupCollapsed() {}
})"#;
