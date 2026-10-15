//! Canonical JavaScript source shared by console runtime adapters.

pub(crate) const CONSOLE_CLASS: &str = r#"((initialize, write, loadUtil) => {
const formatting = () => loadUtil();
// util.format leaves strings as they are when the first has no format specifier, so such
// output needs no util at all.
const plainStrings = (args) =>
  args.every((arg) => typeof arg === "string") &&
  (args.length < 2 || !args[0].includes("%"));
return class Console {
  constructor(stdout, stderr, ignoreErrors) {
    const options = stdout && typeof stdout === "object" &&
      (stdout.stdout || stdout.stderr) ? stdout : null;
    this._stdout = options ? options.stdout : stdout;
    this._stderr = options ? options.stderr : stderr;
    this._inspectOptions = options ? options.inspectOptions : undefined;
    if (!this._stdout) this._stdout = globalThis?.process?.stdout;
    if (!this._stderr) this._stderr = this._stdout;
    initialize(this, options, ignoreErrors);
    // Like Node, bind the public methods to this instance, read through it so subclass
    // overrides win, keeping each method's name.
    for (const key of Object.getOwnPropertyNames(Console.prototype)) {
      if (key === "constructor" || key.startsWith("_")) continue;
      this[key] = this[key].bind(this);
      Object.defineProperty(this[key], "name", { value: key });
    }
  }
  _format(output, args) {
    const configured = this._inspectOptions &&
      typeof this._inspectOptions.get === "function"
      ? this._inspectOptions.get(output)
      : this._inspectOptions;
    if (plainStrings(args)) return args.join(" ");
    const { format, formatWithOptions } = formatting();
    if (configured && typeof formatWithOptions === "function") {
      return formatWithOptions(configured, ...args);
    }
    return typeof format === "function" ? format(...args) : args.join(" ");
  }
  log(...args) {
    const output = this._stdout || process?.stdout;
    write(this, true, this._format(output, args));
    if (!this._tickPending) {
      this._tickPending = true;
      const tick = globalThis?.process?.nextTick;
      if (typeof tick === "function") tick(() => { this._tickPending = false; });
    }
  }
  info(...args) { this.log(...args); }
  dir(...args) { this.log(...args); }
  warn(...args) {
    const output = this._stderr || process?.stderr;
    write(this, false, this._format(output, args));
  }
  error(...args) { this.warn(...args); }
  trace(...args) { this.error(...args); }
  assert(condition, ...args) { if (!condition) this.error(...args); }
  clear() {}
  group() {}
  groupEnd() {}
  table(...args) { this.log(...args); }
  debug(...args) { this.log(...args); }
  dirxml(...args) { this.log(...args); }
  groupCollapsed() {}
};
})"#;
