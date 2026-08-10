{
  const __quenchEnvError = () => {
    const error = new TypeError(
      'The "content" argument must be of type string'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    return error;
  };
  const __quenchEnvValue = (raw) => {
    const value = raw.trim();
    const quote = value[0];
    if (!["'", '"', "`"].includes(quote)) {
      const comment = value.indexOf("#");
      return (comment < 0 ? value : value.slice(0, comment)).trim();
    }
    const end = value.indexOf(quote, 1);
    if (end < 0) return value;
    const result = value.slice(1, end);
    return quote === '"' ? result.replace(/\\n/g, "\n") : result;
  };
  const __quenchParseEnv = (content) => {
    if (typeof content !== "string") throw __quenchEnvError();
    const result = Object.create(null);
    const lines = content.split(/\r?\n/);
    for (let index = 0; index < lines.length; index++) {
      let line = lines[index].trim();
      if (!line || line.startsWith("#")) continue;
      line = line.replace(/^export\s+/, "");
      const match = line.match(/^([^=\s]+)\s*=\s*(.*)$/);
      if (!match) continue;
      const [, key, initialValue] = match;
      let value = initialValue;
      const quote = value.trim()[0];
      if (["'", '"', "`"].includes(quote) && value.trim().length > 1) {
        while (
          value.trim().indexOf(quote, 1) === -1 &&
          index + 1 < lines.length
        ) {
          value += `\n${lines[++index]}`;
        }
      }
      result[key] = __quenchEnvValue(value);
    }
    return result;
  };
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const normalized = String(name).replace(/^node:/, "");
      let result = originalRequire(name);
      if (normalized === "os") {
        result.availableParallelism ||= () => 1;
        result.getPriority ||= () => 0;
        result.setPriority ||= () => undefined;
        result.machine ||= () => "unknown";
        result.version ||= () => "";
      }
      if (normalized === "util") {
        result.parseEnv ||= __quenchParseEnv;
      }
      if (normalized === "console") {
        result.createTask ||= () => ({});
        result.dir ||= () => undefined;
        result.time ||= () => undefined;
        result.timeEnd ||= () => undefined;
        result.assert ||= () => undefined;
        result.table ||= () => undefined;
      }
      if (normalized === "dgram") {
        result = Object.assign({}, result);
        result.createSocket ||= () => undefined;
        result.Socket ||= function Socket() {};
        result.SocketAddress ||= function SocketAddress() {};
      }
      return result;
    };
  }
}
