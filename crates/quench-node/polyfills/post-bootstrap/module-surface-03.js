const __quenchAddStreamAliases = (result) => {
  result.Stream ||= result.Readable;
  result.Writable ||= result.Readable;
  result.Duplex ||= result.Transform;
};
const __quenchAddStreamWebCompat = (result) => {
  for (const name of ["Readable", "Writable", "Duplex"]) {
    result[name].toWeb ||= () => ({});
    result[name].fromWeb ||= (value) => value;
  }
};
const __quenchAddStreamDefaults = (result) => {
  result.pipeline ||= () => undefined;
  result.finished ||= () => undefined;
  result.addAbortSignal ||= () => undefined;
  result.compose ||= (stream) => stream;
  result.setDefaultHighWaterMark ||= () => 16384;
  result.getDefaultHighWaterMark ||= () => 16384;
};
const __quenchMakeCallableConstructor = (Constructor) => {
  if (Constructor.__quenchCallable) return Constructor;
  const callable = function (...args) {
    return new Constructor(...args);
  };
  callable.prototype = Constructor.prototype;
  Object.setPrototypeOf(callable, Constructor);
  Object.defineProperty(callable, "__quenchCallable", { value: true });
  return callable;
};
const __quenchValidatePipeline = (pipeline, args) => {
  const callback = args[args.length - 1];
  if (args.length === 0) {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: The last argument must be of type function"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (args.length < 3 || typeof callback !== "function") {
    const error = new TypeError(
      "ERR_MISSING_ARGS: The pipeline requires at least two streams"
    );
    error.code = "ERR_MISSING_ARGS";
    throw error;
  }
  const result = pipeline(...args);
  return result === undefined ? args[args.length - 2] : result;
};
const __quenchAddStreamCompat = (result) => {
  __quenchAddStreamAliases(result);
  result.Writable = __quenchMakeCallableConstructor(result.Writable);
  __quenchAddStreamWebCompat(result);
  __quenchAddStreamDefaults(result);
  const pipeline = result.pipeline;
  result.pipeline = (...args) => __quenchValidatePipeline(pipeline, args);
  result.promises ||= globalThis.require("stream/promises");
  const promisifyCustom = Symbol.for("nodejs.util.promisify.custom");
  result.pipeline[promisifyCustom] = result.promises.pipeline;
  result.finished[promisifyCustom] = result.promises.finished;
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) => {
    const result = originalRequire(name);
    if (String(name).replace(/^node:/, "") === "stream")
      return __quenchAddStreamCompat(result);
    return result;
  };
}
