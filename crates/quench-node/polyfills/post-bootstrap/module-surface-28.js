const __quenchClusterStateFallbacks = (result) => {
  result.isPrimary ??= true;
  result.isWorker ??= false;
  result.worker ??= undefined;
  result.workers ||= {};
  result.settings ||= {};
};
const __quenchClusterMethodFallbacks = (result) => {
  result.fork ||= () => undefined;
  result.setupPrimary ||= () => undefined;
  result.disconnect ||= () => undefined;
  result.schedulingPolicy ??= 2;
  result.Worker ||= function Worker() {};
};
const __quenchClusterFallbacks = (result) => {
  __quenchClusterStateFallbacks(result);
  __quenchClusterMethodFallbacks(result);
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
