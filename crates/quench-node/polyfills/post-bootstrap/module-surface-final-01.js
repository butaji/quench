const __quenchTestModuleFallbacks = (result, originalRequire, name) => {
  let runner;
  try {
    runner = originalRequire(name);
  } catch (_) {
    runner = function test() {};
  }
  for (const exportName of [
    "test",
    "describe",
    "it",
    "before",
    "after",
    "beforeEach",
    "afterEach"
  ])
    runner[exportName] ||= () => undefined;
  runner.run ||= () => ({});
  runner.mock ||= {};
  runner.snapshot ||= () => undefined;
  return runner;
};
const __quenchUtilTypesBasicFallbacks = (result) => {
  result.isAnyArrayBuffer ||= () => false;
  result.isArgumentsObject ||= (value) =>
    Object.prototype.toString.call(value) === "[object Arguments]";
  result.isArrayBuffer ||= (value) =>
    Object.prototype.toString.call(value) === "[object ArrayBuffer]";
  result.isArrayBufferView ||= (value) => value && ArrayBuffer.isView(value);
  result.isAsyncFunction ||= (value) =>
    Object.prototype.toString.call(value) === "[object AsyncFunction]";
};
const __quenchUtilTypesCollectionFallbacks = (result) => {
  result.isDate ||= (value) => value instanceof Date;
  result.isMap ||= (value) => value instanceof Map;
  result.isPromise ||= (value) => value instanceof Promise;
  result.isRegExp ||= (value) => value instanceof RegExp;
  result.isSet ||= (value) => value instanceof Set;
};
const __quenchUtilTypesTypedFallbacks = (result) => {
  result.isTypedArray ||= (value) =>
    value && ArrayBuffer.isView(value) && !(value instanceof DataView);
  result.isUint8Array ||= (value) => value instanceof Uint8Array;
};
const __quenchUtilTypesFallbacks = (result) => {
  __quenchUtilTypesBasicFallbacks(result);
  __quenchUtilTypesCollectionFallbacks(result);
  __quenchUtilTypesTypedFallbacks(result);
  return result;
};
const __quenchApplyFinalModule01 = (name, originalRequire) => {
  const normalized = String(name).replace(/^node:/, "");
  if (normalized === "internal/streams/end-of-stream")
    return {
      kEosNodeSynchronousCallback: Symbol("kEosNodeSynchronousCallback")
    };
  if (normalized === "sqlite")
    return {
      DatabaseSync: function DatabaseSync() {},
      StatementSync: function StatementSync() {},
      constants: {}
    };
  if (normalized === "inspector")
    return {
      open: () => undefined,
      close: () => undefined,
      url: () => undefined,
      waitForDebugger: () => undefined,
      Session: function Session() {},
      console: {}
    };
  const result = originalRequire(name);
  if (normalized === "http") __quenchAddHttpEvents(result);
  if (normalized === "test")
    return __quenchTestModuleFallbacks(result, originalRequire, name);
  if (normalized === "util") {
    result.types ||= Object.create(null);
    return result;
  }
  if (normalized === "util/types") {
    const util = originalRequire("util");
    util.types ||= result;
    return __quenchUtilTypesFallbacks(util.types);
  }
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) =>
    __quenchApplyFinalModule01(name, originalRequire);
}
