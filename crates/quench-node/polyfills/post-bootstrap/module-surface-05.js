{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "os") {
        result.availableParallelism ||= () => 1;
        result.getPriority ||= () => 0;
        result.setPriority ||= () => undefined;
        result.machine ||= () => "unknown";
        result.version ||= () => "";
      }
      return result;
    };
  }
}
