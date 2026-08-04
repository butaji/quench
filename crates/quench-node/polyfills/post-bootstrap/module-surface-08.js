{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "perf_hooks") {
        result.PerformanceEntry ||= function PerformanceEntry() {};
        result.PerformanceMark ||= function PerformanceMark() {};
        result.PerformanceMeasure ||= function PerformanceMeasure() {};
        result.monitorEventLoopDelay ||= () => ({});
        result.createHistogram ||= () => ({});
        result.constants ||= {};
      }
      return result;
    };
  }
}
