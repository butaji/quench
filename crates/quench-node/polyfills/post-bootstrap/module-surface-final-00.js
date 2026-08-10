const __quenchTestReporterFallbacks = () =>
  Object.fromEntries(
    "dot junit json lcov markdown spec tap teamcity xunit"
      .split(" ")
      .map((name) => [name, () => undefined])
  );
const __quenchInspectorPromisesFallbacks = () => ({
  open: async () => undefined,
  close: async () => undefined,
  url: async () => undefined,
  waitForDebugger: async () => undefined,
  Session: function Session() {}
});
const __quenchStreamWebFallbacks = (result, originalRequire, name) => {
  result = globalThis.__quenchFsPromisesModule || Object.assign({}, result);
  globalThis.__quenchFsConstantsModule ||= originalRequire("fs").constants;
  result.constants = globalThis.__quenchFsConstantsModule;
  for (const constructor of "ReadableStream ReadableStreamDefaultReader ReadableStreamBYOBReader ReadableStreamBYOBRequest ReadableByteStreamController ReadableStreamDefaultController TransformStream TransformStreamDefaultController WritableStream WritableStreamDefaultWriter WritableStreamDefaultController ByteLengthQueuingStrategy CountQueuingStrategy TextEncoderStream TextDecoderStream CompressionStream DecompressionStream".split(
    " "
  )) {
    if (typeof globalThis[constructor] === "function") {
      result[constructor] = globalThis[constructor];
    }
  }
  result.ReadableStream ||= function ReadableStream() {};
  result.ReadableStream.prototype ||=
    originalRequire(name).ReadableStream.prototype;
  result.ReadableStream.from ||= async function* (source) {
    yield* source;
  };
  for (const constructor of "WritableStream TransformStream ReadableStreamDefaultReader WritableStreamDefaultWriter ByteLengthQueuingStrategy CountQueuingStrategy".split(
    " "
  )) {
    result[constructor] ||= function Constructor() {};
  }
  return result;
};
const __quenchApplyFinalModule00 = (name, result, originalRequire) => {
  const normalized = String(name).replace(/^node:/, "");
  if (
    normalized === "diagnostics_channel" &&
    globalThis.__nodeDiagnosticsChannel
  ) {
    return globalThis.__nodeDiagnosticsChannel;
  }
  if (normalized === "test/reporters") return __quenchTestReporterFallbacks();
  if (normalized === "inspector/promises") {
    return __quenchInspectorPromisesFallbacks();
  }
  if (normalized === "stream/web") {
    return __quenchStreamWebFallbacks(result, originalRequire, name);
  }
  if (normalized === "fs/promises") {
    result = globalThis.__nodeFs?.promises || Object.assign({}, result);
    for (const name of ["open", "appendFile", "writeFile"]) {
      if (typeof globalThis.__nodeFs?.promises?.[name] === "function") {
        result[name] = globalThis.__nodeFs.promises[name];
      }
    }
    result.FileHandle ||= function FileHandle() {};
    result.mkdtempDisposable ||= async (prefix, options) => {
      const path = globalThis.__nodeFs.mkdtempSync(prefix, options);
      const removalPath = globalThis.__nodePath.resolve(path);
      let removed = false;
      const remove = async () => {
        if (removed) return;
        removed = true;
        try {
          globalThis.__nodeFs.rmdirSync(removalPath);
        } catch (error) {
          removed = false;
          throw error;
        }
      };
      Symbol.asyncDispose ||= Symbol("Symbol.asyncDispose");
      return { path, remove, [Symbol.asyncDispose]: remove };
    };
    globalThis.__quenchFsPromisesModule = result;
  }
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) => {
    const normalized = String(name).replace(/^node:/, "");
    const result =
      normalized === "test/reporters" || normalized === "inspector/promises"
        ? undefined
        : originalRequire(name);
    return __quenchApplyFinalModule00(name, result, originalRequire);
  };
}
