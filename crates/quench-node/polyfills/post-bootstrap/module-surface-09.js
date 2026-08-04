{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "console") {
        result.createTask ||= () => ({});
        result.dir ||= () => undefined;
        result.time ||= () => undefined;
        result.timeEnd ||= () => undefined;
        result.assert ||= () => undefined;
        result.table ||= () => undefined;
      }
      return result;
    };
  }
}
