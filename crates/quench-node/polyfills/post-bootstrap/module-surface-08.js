{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "perf_hooks") {
        for (const name of "PerformanceEntry PerformanceMark PerformanceMeasure".split(
          " "
        )) {
          result[name] ||= function Constructor() {};
        }
        result.monitorEventLoopDelay ||= () => ({});
        result.createHistogram ||= () => ({});
        result.constants ||= {};
      }
      return result;
    };
  }
}
