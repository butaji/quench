const __quenchTestReporterFallbacks = () =>
  Object.fromEntries(
    [
      "dot",
      "junit",
      "json",
      "lcov",
      "markdown",
      "spec",
      "tap",
      "teamcity",
      "xunit"
    ].map((name) => [name, () => undefined])
  );
const __quenchInspectorPromisesFallbacks = () => ({
  open: async () => undefined,
  close: async () => undefined,
  url: async () => undefined,
  waitForDebugger: async () => undefined,
  Session: function Session() {}
});
const __quenchStreamWebFallbacks = (result, originalRequire, name) => {
  result = Object.assign({}, result);
  result.ReadableStream = Object.assign(
    function ReadableStream() {},
    result.ReadableStream
  );
  result.ReadableStream.prototype =
    originalRequire(name).ReadableStream.prototype;
  result.ReadableStream.from ||= async function* (source) {
    yield* source;
  };
  for (const constructor of [
    "WritableStream",
    "TransformStream",
    "ReadableStreamDefaultReader",
    "WritableStreamDefaultWriter",
    "ByteLengthQueuingStrategy",
    "CountQueuingStrategy"
  ])
    result[constructor] ||= function Constructor() {};
  return result;
};
const __quenchApplyFinalModule00 = (name, result, originalRequire) => {
  const normalized = String(name).replace(/^node:/, "");
  if (normalized === "test/reporters") return __quenchTestReporterFallbacks();
  if (normalized === "inspector/promises")
    return __quenchInspectorPromisesFallbacks();
  if (normalized === "stream/web")
    return __quenchStreamWebFallbacks(result, originalRequire, name);
  if (normalized === "fs/promises") {
    result = Object.assign({}, result);
    result.FileHandle ||= function FileHandle() {};
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
