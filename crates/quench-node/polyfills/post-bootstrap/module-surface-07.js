{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "timers/promises") {
        result.scheduler ||= {
          wait: async () => undefined,
          yield: async () => undefined
        };
      }
      return result;
    };
  }
}
