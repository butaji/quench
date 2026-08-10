const __quenchClusterFallbacks = (result) => {
  result.isPrimary ??= true;
  result.isWorker ??= false;
  result.worker ??= undefined;
  result.workers ||= {};
  result.settings ||= {};
  result.fork ||= () => undefined;
  result.setupPrimary ||= () => undefined;
  result.disconnect ||= () => undefined;
  result.schedulingPolicy ??= 2;
  result.Worker ||= function Worker() {};
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) => {
    const result = originalRequire(name);
    if (String(name).replace(/^node:/, "") === "cluster") {
      return __quenchClusterFallbacks(result);
    }
    return result;
  };
}
