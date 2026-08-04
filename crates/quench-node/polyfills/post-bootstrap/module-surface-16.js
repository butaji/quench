{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "https") {
        result = Object.assign({}, result);
        result.request ||= () => undefined;
        result.get ||= () => undefined;
        result.createServer ||= () => undefined;
        result.Agent ||= function Agent() {};
        result.Server ||= function Server() {};
        result.globalAgent ||= {};
      }
      return result;
    };
  }
}
