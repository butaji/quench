{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "repl") {
        result = Object.assign({}, result);
        result.start ||= () => ({});
        result.recoverable ||= () => false;
        result.REPLServer ||= function REPLServer() {};
      }
      return result;
    };
  }
}
