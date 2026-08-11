//! Polyfill: `context`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchVmContexts = new WeakSet();
const __quenchVmTypeError = (message) => {
  throw Object.assign(new TypeError(message), { code: "ERR_INVALID_ARG_TYPE" });
};
const __quenchVmRangeError = (message) => {
  throw Object.assign(new RangeError(message), { code: "ERR_OUT_OF_RANGE" });
};
const __quenchVmInvalidTypeSuffix = (value) =>
  value === null
    ? " Received null"
    : typeof value === "object"
    ? ` Received an instance of ${value.constructor?.name || "Object"}`
    : ` Received type ${typeof value} (${
      typeof value === "string" ? `'${value}'` : String(value)
    })`;
const __quenchVmCacheMatches = (data, code) => {
  const bytes = ArrayBuffer.isView(data)
    ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
    : data;
  return NodeBuffer.from(bytes).toString() === code;
};
const __quenchVmCheckRestrictedDeclaration = (code) => {
  const declaration = /\blet\s+([A-Za-z_$][\w$]*)/.exec(String(code));
  if (!declaration) return;
  const descriptor = Object.getOwnPropertyDescriptor(
    globalThis,
    declaration[1],
  );
  if (descriptor?.configurable === false) {
    throw new SyntaxError(`${declaration[1]} has already been declared`);
  }
};
const __quenchVmIsContext = (value) => {
  if (
    value === null ||
    (typeof value !== "object" && typeof value !== "function")
  ) {
    __quenchVmTypeError('The "sandbox" argument must be of type object.');
  }
  return __quenchVmContexts.has(value);
};
const __quenchVmSourceMapURL = (code) =>
  /\/\/#[ \t]*sourceMappingURL=([^\s]+)/.exec(code)?.[1];
const __quenchVmContextValue = (value, sandbox) => {
  if (value === sandbox) return globalThis;
  if (value === null || typeof value !== "object") return value;
  const clone = Array.isArray(value)
    ? []
    : Object.create(Object.getPrototypeOf(value));
  for (const key of Reflect.ownKeys(value)) {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if ("value" in descriptor && descriptor.value === sandbox) {
      descriptor.value = globalThis;
    }
    Object.defineProperty(clone, key, descriptor);
  }
  return clone;
};
const __quenchVmApplyScriptCache = (script, options) => {
  if (options?.produceCachedData) {
    script.cachedDataProduced = true;
    script.cachedData = NodeBuffer.from(script.code);
  }
  if (options?.cachedData) {
    script.cachedDataRejected = !__quenchVmCacheMatches(
      options.cachedData,
      script.code,
    );
  }
};
const __quenchVmValidateOffset = (options, key) => {
  if (options[key] === undefined) return;
  if (typeof options[key] !== "number") {
    __quenchVmTypeError(`The ${key} option must be an integer`);
  }
  if (
    !Number.isInteger(options[key]) ||
    options[key] < 0 ||
    options[key] > 2 ** 32 - 1
  ) {
    __quenchVmRangeError(`The ${key} option is out of range`);
  }
};
const __quenchVmValidateTimeout = (options) => {
  if (options.timeout === undefined) return;
  if (typeof options.timeout !== "number") {
    __quenchVmTypeError("The timeout option must be a number");
  }
  if (!Number.isFinite(options.timeout) || options.timeout <= 0) {
    __quenchVmRangeError("The timeout option is out of range");
  }
};
const __quenchVmValidateBoolean = (options, key) => {
  if (options[key] !== undefined && typeof options[key] !== "boolean") {
    __quenchVmTypeError(`The ${key} option must be a boolean`);
  }
};
const __quenchVmValidateScriptFields = (options) => {
  if (options.filename !== undefined && typeof options.filename !== "string") {
    __quenchVmTypeError("The filename option must be a string");
  }
  if (
    options.produceCachedData !== undefined &&
    typeof options.produceCachedData !== "boolean"
  ) {
    __quenchVmTypeError("The produceCachedData option must be a boolean");
  }
  if (
    options.cachedData !== undefined &&
    !ArrayBuffer.isView(options.cachedData)
  ) {
    __quenchVmTypeError("The cachedData option must be a Buffer");
  }
};
const __quenchVmValidateContextOptions = (options, allowFilename) => {
  if (options === undefined) return;
  if (allowFilename && typeof options === "string") return;
  if (
    options === null ||
    typeof options !== "object" ||
    Array.isArray(options)
  ) {
    __quenchVmTypeError(
      `The "options" argument must be of type object.${
        __quenchVmInvalidTypeSuffix(
          options,
        )
      }`,
    );
  }
  for (const key of ["name", "origin", "contextName", "contextOrigin"]) {
    if (options[key] !== undefined && typeof options[key] !== "string") {
      __quenchVmTypeError(
        `The "options.${key}" property must be of type string. Received null`,
      );
    }
  }
};
const __quenchVmValidateCompileFields = (options) => {
  if (options.filename === null) {
    __quenchVmTypeError(
      'The "options.filename" property must be of type string. Received null',
    );
  }
  if (options.columnOffset === null) {
    __quenchVmTypeError(
      'The "options.columnOffset" property must be of type number. Received null',
    );
  }
  if (options.lineOffset === null) {
    __quenchVmTypeError(
      'The "options.lineOffset" property must be of type number. Received null',
    );
  }
  if (options.cachedData === null) {
    __quenchVmTypeError(
      'The "options.cachedData" property must be an instance of Buffer, TypedArray, or DataView. Received null',
    );
  }
  if (options.produceCachedData === null) {
    __quenchVmTypeError(
      'The "options.produceCachedData" property must be of type boolean. Received null',
    );
  }
  if (
    options.parsingContext !== undefined &&
    !__quenchVmContexts.has(options.parsingContext)
  ) {
    __quenchVmTypeError(
      `The "options.parsingContext" property must be an instance of Context.${
        __quenchVmInvalidTypeSuffix(
          options.parsingContext,
        )
      }`,
    );
  }
};
const __quenchVmValidateExtensions = (extensions) => {
  if (!Array.isArray(extensions)) {
    __quenchVmTypeError(
      'The "options.contextExtensions" property must be an instance of Array. Received null',
    );
  }
  for (const [index, extension] of extensions.entries()) {
    if (extension === null || typeof extension !== "object") {
      __quenchVmTypeError(
        `The "options.contextExtensions[${index}]" property must be of type object.${
          __quenchVmInvalidTypeSuffix(
            extension,
          )
        }`,
      );
    }
  }
};
const __quenchVmValidateCompileOptions = (options) => {
  if (options === undefined) return;
  if (
    options === null ||
    typeof options !== "object" ||
    Array.isArray(options)
  ) {
    __quenchVmTypeError(
      `The "options" argument must be of type object.${
        __quenchVmInvalidTypeSuffix(
          options,
        )
      }`,
    );
  }
  __quenchVmValidateCompileFields(options);
  if (options.contextExtensions !== undefined) {
    __quenchVmValidateExtensions(options.contextExtensions);
  }
};
const __quenchVmValidateScriptOptions = (options) => {
  if (options === undefined) return;
  if (
    options === null ||
    typeof options !== "object" ||
    Array.isArray(options)
  ) {
    __quenchVmTypeError("The options argument must be an object");
  }
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
  code,
) =>
  code
    ? `${filename}:${
      lineOffset + 1
    }\n${code}\n ^\n\n${error.name}: ${error.message}\nat ${filename}:${
      lineOffset + 1
    }:${columnOffset + 7}`
    : `${filename}:${lineOffset + 1}:${columnOffset + 8}\n ^\n${error.stack}`;
const __quenchVmRestoreProperties = (
  keys,
  previous,
  hiddenProcess,
  previousPrototype,
) => {
  for (const key of keys) {
    const descriptor = previous.get(key);
    if (descriptor?.configurable) {
      Object.defineProperty(globalThis, key, descriptor);
    } else Reflect.deleteProperty(globalThis, key);
  }
  if (hiddenProcess) {
    Object.defineProperty(globalThis, "process", hiddenProcess);
  }
  if (previousPrototype) Object.setPrototypeOf(globalThis, previousPrototype);
};
const __quenchVmInstallContext = (sandbox) => {
  const keys = [
    ...Object.getOwnPropertyNames(sandbox),
    ...Object.getOwnPropertySymbols(sandbox),
  ];
  const previous = new Map();
  const originalGlobalKeys = new Set([
    ...Object.getOwnPropertyNames(globalThis),
    ...Object.getOwnPropertySymbols(globalThis),
  ]);
  const hiddenProcess = !keys.includes("process") &&
    Object.getOwnPropertyDescriptor(globalThis, "process");
  if (hiddenProcess) Reflect.deleteProperty(globalThis, "process");
  const previousPrototype = Object.getPrototypeOf(globalThis);
  const sandboxPrototype = Object.getPrototypeOf(sandbox);
  if (sandboxPrototype) Object.setPrototypeOf(globalThis, sandboxPrototype);
  for (const key of keys) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    const descriptor = Object.getOwnPropertyDescriptor(sandbox, key);
    if ("value" in descriptor) {
      descriptor.value = __quenchVmContextValue(descriptor.value, sandbox);
    }
    if (!previous.get(key) || previous.get(key).configurable) {
      Object.defineProperty(globalThis, key, descriptor);
    }
    if (key === "setTimeout" && typeof descriptor.value === "function") {
      const timer = descriptor.value;
      globalThis.setTimeout = (callback, ...args) =>
        timer(() => __quenchVmRunCallback(callback, sandbox, []), ...args);
    }
  }
  return {
    keys,
    previous,
    originalGlobalKeys,
    hiddenProcess,
    previousPrototype,
  };
};
const __quenchVmRestoreNewContext = (
  original,
  keys,
  preservesHostGlobals,
  hidesProcess,
) => {
  if (hidesProcess && original.has("process")) {
    Object.defineProperty(globalThis, "process", original.get("process"));
  }
  if (!preservesHostGlobals) {
    for (
      const key of [
        ...Object.getOwnPropertyNames(globalThis),
        ...Object.getOwnPropertySymbols(globalThis),
      ]
    ) {
      if (!original.has(key)) Reflect.deleteProperty(globalThis, key);
    }
    for (const [key, descriptor] of original) {
      Object.defineProperty(globalThis, key, descriptor);
    }
    return;
  }
  for (const key of keys) {
    if (original.has(key)) {
      Object.defineProperty(globalThis, key, original.get(key));
    } else Reflect.deleteProperty(globalThis, key);
  }
};
const __quenchVmSnapshotGlobals = () =>
  new Map(
    Object.getOwnPropertyNames(globalThis).map((key) => [
      key,
      Object.getOwnPropertyDescriptor(globalThis, key),
    ]),
  );
const __quenchVmRunInNewContext = (code, sandbox, options) => {
  __quenchVmValidateContextOptions(options, true);
  if (!__quenchVmIsObject(sandbox)) {
    __quenchVmTypeError("The context argument must be an object");
  }
  const keys = Object.getOwnPropertyNames(sandbox).concat(
    Object.getOwnPropertySymbols(sandbox),
  );
  const hidesProcess = !keys.includes("process");
  const preservesHostGlobals = keys.some(
    (key) => typeof sandbox[key] === "function",
  );
  const original = __quenchVmSnapshotGlobals();
  if (hidesProcess) Reflect.deleteProperty(globalThis, "process");
  try {
    for (const key of keys) globalThis[key] = sandbox[key];
    const result = __quenchVmEvaluateContext(code, sandbox, options, {
      keys,
      originalGlobalKeys: new Set(original.keys()),
      formatCode: true,
    });
    if (typeof result === "function") {
      Object.setPrototypeOf(result, Object.create(Function.prototype));
    }
    if (!keys.includes("Proxy") && /\bProxy\b/.test(String(code))) {
      sandbox.Proxy = function (...args) {
        return new globalThis.Proxy(...args);
      };
    }
    return result;
  } finally {
    __quenchVmRestoreNewContext(
      original,
      keys,
      preservesHostGlobals,
      hidesProcess,
    );
  }
};
"#);
