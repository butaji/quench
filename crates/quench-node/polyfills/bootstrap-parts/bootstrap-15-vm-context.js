const __quenchVmTypeError = (message) => {
  const error = new TypeError(message);
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
const __quenchVmRangeError = (message) => {
  const error = new RangeError(message);
  error.code = "ERR_OUT_OF_RANGE";
  throw error;
};
const __quenchVmValidateOffset = (options, key) => {
  if (options[key] === undefined) return;
  if (typeof options[key] !== "number")
    __quenchVmTypeError(`The ${key} option must be an integer`);
  if (
    !Number.isInteger(options[key]) ||
    options[key] < 0 ||
    options[key] > 2 ** 32 - 1
  )
    __quenchVmRangeError(`The ${key} option is out of range`);
};
const __quenchVmValidateTimeout = (options) => {
  if (options.timeout === undefined) return;
  if (typeof options.timeout !== "number")
    __quenchVmTypeError("The timeout option must be a number");
  if (!Number.isFinite(options.timeout) || options.timeout <= 0)
    __quenchVmRangeError("The timeout option is out of range");
};
const __quenchVmValidateBoolean = (options, key) => {
  if (options[key] !== undefined && typeof options[key] !== "boolean")
    __quenchVmTypeError(`The ${key} option must be a boolean`);
};
const __quenchVmValidateScriptFields = (options) => {
  if (options.filename !== undefined && typeof options.filename !== "string")
    __quenchVmTypeError("The filename option must be a string");
  if (
    options.produceCachedData !== undefined &&
    typeof options.produceCachedData !== "boolean"
  )
    __quenchVmTypeError("The produceCachedData option must be a boolean");
  if (
    options.cachedData !== undefined &&
    !ArrayBuffer.isView(options.cachedData)
  )
    __quenchVmTypeError("The cachedData option must be a Buffer");
};
const __quenchVmValidateScriptOptions = (options) => {
  if (options === undefined) return;
  if (options === null || typeof options !== "object" || Array.isArray(options))
    __quenchVmTypeError("The options argument must be an object");
  __quenchVmValidateOffset(options, "lineOffset");
  __quenchVmValidateOffset(options, "columnOffset");
  __quenchVmValidateScriptFields(options);
  __quenchVmValidateTimeout(options);
  __quenchVmValidateBoolean(options, "displayErrors");
  __quenchVmValidateBoolean(options, "breakOnSigint");
};
const __quenchVmFormatStack = (
  error,
  filename,
  lineOffset,
  columnOffset,
  code
) =>
  code
    ? `${filename}:${lineOffset + 1}\n${code}\n ^\n\n${error.name}: ${error.message}\nat ${filename}:${lineOffset + 1}:${columnOffset + 7}`
    : `${filename}:${lineOffset + 1}:${columnOffset + 8}\n ^\n${error.stack}`;
const __quenchVmRestoreProperties = (keys, previous, hiddenProcess) => {
  for (const key of keys) {
    const descriptor = previous.get(key);
    if (descriptor?.configurable)
      Object.defineProperty(globalThis, key, descriptor);
    else Reflect.deleteProperty(globalThis, key);
  }
  if (hiddenProcess)
    Object.defineProperty(globalThis, "process", hiddenProcess);
};
const __quenchVmInstallContext = (sandbox) => {
  const keys = Object.getOwnPropertyNames(sandbox);
  const previous = new Map();
  const originalGlobalKeys = new Set(Object.getOwnPropertyNames(globalThis));
  const hiddenProcess =
    !keys.includes("process") &&
    Object.getOwnPropertyDescriptor(globalThis, "process");
  if (hiddenProcess) Reflect.deleteProperty(globalThis, "process");
  for (const key of keys) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    const descriptor = Object.getOwnPropertyDescriptor(sandbox, key);
    if (!previous.get(key) || previous.get(key).configurable)
      Object.defineProperty(globalThis, key, descriptor);
    if (key === "setTimeout" && typeof descriptor.value === "function") {
      const timer = descriptor.value;
      globalThis.setTimeout = (callback, ...args) =>
        timer(() => __quenchVmRunCallback(callback, sandbox, []), ...args);
    }
  }
  return { keys, previous, originalGlobalKeys, hiddenProcess };
};
const __quenchVmRestoreNewContext = (
  original,
  keys,
  preservesHostGlobals,
  hidesProcess
) => {
  if (hidesProcess && original.has("process"))
    Object.defineProperty(globalThis, "process", original.get("process"));
  if (!preservesHostGlobals) {
    for (const key of Object.getOwnPropertyNames(globalThis))
      if (!original.has(key)) Reflect.deleteProperty(globalThis, key);
    for (const [key, descriptor] of original)
      Object.defineProperty(globalThis, key, descriptor);
    return;
  }
  for (const key of keys) {
    if (original.has(key))
      Object.defineProperty(globalThis, key, original.get(key));
    else Reflect.deleteProperty(globalThis, key);
  }
};
const __quenchVmRunInNewContext = (code, sandbox, options) => {
  if (!__quenchVmIsObject(sandbox))
    __quenchVmTypeError("The context argument must be an object");
  const keys = Object.getOwnPropertyNames(sandbox);
  const hidesProcess = !keys.includes("process");
  const preservesHostGlobals = keys.some(
    (key) => typeof sandbox[key] === "function"
  );
  const original = new Map(
    Object.getOwnPropertyNames(globalThis).map((key) => [
      key,
      Object.getOwnPropertyDescriptor(globalThis, key)
    ])
  );
  if (hidesProcess) Reflect.deleteProperty(globalThis, "process");
  try {
    for (const key of keys) globalThis[key] = sandbox[key];
    const result = __quenchVmEvaluateContext(code, sandbox, options, {
      keys,
      originalGlobalKeys: new Set(original.keys()),
      formatCode: true
    });
    return result;
  } finally {
    __quenchVmRestoreNewContext(
      original,
      keys,
      preservesHostGlobals,
      hidesProcess
    );
  }
};
