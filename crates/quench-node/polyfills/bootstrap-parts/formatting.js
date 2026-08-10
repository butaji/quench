globalThis.__nodeUtil = {
  TextEncoder: globalThis.TextEncoder,
  TextDecoder: globalThis.TextDecoder,
  isArray: (value) => Array.isArray(value),
  debuglog: () => () => {},
  deprecate: __nodeUtilDeprecate,
  pendingDeprecate: __nodeUtilPendingDeprecate,
  _extend: (target, source) => {
    if (source && typeof source === "object") Object.assign(target, source);
    return target;
  },
  toUSVString: (value) => {
    const input = String(value);
    let output = "";
    for (let index = 0; index < input.length; index++) {
      const code = input.charCodeAt(index);
      if (code >= 0xd800 && code <= 0xdbff) {
        const next = input.charCodeAt(index + 1);
        if (next >= 0xdc00 && next <= 0xdfff) {
          output += input[index++] + input[index];
        } else output += "\ufffd";
      } else if (code >= 0xdc00 && code <= 0xdfff) output += "\ufffd";
      else output += input[index];
    }
    return output;
  },
  stripVTControlCharacters: (value) => {
    if (typeof value !== "string") {
      const received =
        value !== null && typeof value === "object"
          ? ` Received an instance of ${value.constructor?.name || "Object"}`
          : ` Received type ${typeof value} (${String(value)})`;
      throw Object.assign(new TypeError('The "str" argument must be of type string.' + received), { code: "ERR_INVALID_ARG_TYPE" });
    }
    return value
      .replace(/\u001b\][\s\S]*?(?:\u0007|\u001b\\|\u009c)/g, "")
      .replace(
        /[\u001b\u009b][[\]()#;?]*(?:(?:(?:[a-zA-Z\d]*(?:;[-a-zA-Z\d\/#&.:=?%@~_]+)*)?\u0007)|(?:(?:\d{1,4}(?:;\d{0,4})*)?[\dA-PR-TZcf-nq-uy=><~]))/g,
        ""
      );
  },
  promisify:
    (fn) =>
    (...args) =>
      new Promise((resolve, reject) =>
        fn(...args, (error, ...values) =>
          error
            ? reject(error)
            : resolve(values.length > 1 ? values : values[0])
        )
      ),
  format: (...args) => {
    if (!args.length) return "";
    if (typeof args[0] !== "string") {
      return args.map((value) => __nodeUtilInspectValue(value)).join(" ");
    }
    let index = 1;
    return (
      args[0].replace(/%[sdifjoOc%]/g, (token) => {
        if (token === "%%") return "%";
        if (token === "%c") {
          if (index >= args.length) return token;
          index++;
          return "";
        }
        if (index >= args.length) return token;
        const value = args[index++];
        return __nodeUtilFormatTokenValue(token, value);
      }) +
      args
        .slice(index)
        .map((value) => ` ${__nodeUtilInspectValue(value)}`)
        .join("")
    );
  },
  // eslint-disable-next-line complexity
  inspect: (value, options = {}, depth, colors) => {
    if (options == null) options = {};
    else if (typeof options === "boolean") {
      options = {
        showHidden: options,
        depth: depth === undefined ? 2 : depth,
        colors: colors === true
      };
    } else if (typeof options === "number") options = { depth: options };
    if (value instanceof Date) return value.toISOString();
    if (typeof value === "symbol") return String(value);
    if (typeof value === "number") return __nodeUtilFormatNumeric(value);
    if (typeof value === "string") {
      return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`;
    }
    if (typeof value === "function") return __nodeUtilInspectFunction(value);
    if (value instanceof Error) return __nodeUtilInspectError(value);
    if (value instanceof NodeBuffer) return __nodeUtilInspectBuffer(value);
    if (
      value &&
      typeof value === "object" &&
      Object.keys(value).some((key) =>
        ["URL", "NodeURL"].includes(value[key]?.constructor?.name)
      )
    ) {
      return `{ ${Object.keys(value)
        .map((key) => `${key}: URL {}`)
        .join(", ")} }`;
    }
    if (options.depth === 0 && value && typeof value === "object") {
      return `{ ${Object.keys(value)
        .map(
          (key) =>
            `${key}: ${
              value[key] && typeof value[key] === "object"
                ? `${value[key].constructor.name} {}`
                : String(value[key])
            }`
        )
        .join(", ")} }`;
    }
    if (
      value &&
      typeof value[Symbol.for("nodejs.util.inspect.custom")] === "function"
    ) {
      return value[Symbol.for("nodejs.util.inspect.custom")](
        options.depth ?? 2,
        options
      );
    }
    if (value && typeof value === "object") {
      const keys = Object.keys(value);
      const entries = keys.map((key) => {
        const renderedKey = /^[A-Za-z_$][\w$]*$/.test(key) ? key : `'${key}'`;
        const item = value[key];
        let rendered;
        if (typeof item === "function") {
          rendered = __nodeUtilInspectFunction(item);
        } else if (item === null) rendered = "null";
        else if (typeof item === "string") rendered = `'${item}'`;
        else if (typeof item === "object") rendered = "[Object]";
        else rendered = String(item);
        return `${renderedKey}: ${rendered}`;
      });
      return `{${entries.length ? ` ${entries.join(", ")} ` : ""}}`;
    }
    try {
      return JSON.stringify(value);
    } catch (_) {
      return String(value);
    }
  },
  getCallSites: () => [
    { scriptName: "", lineNumber: 0 },
    { scriptName: "", lineNumber: 0 }
  ],
  types: {
    isDate: (value) => value instanceof Date,
    isPromise: (value) => value instanceof Promise,
    isBooleanObject: (value) => value instanceof Boolean,
    isNumberObject: (value) => value instanceof Number,
    isStringObject: (value) => value instanceof String,
    isSymbolObject: (value) =>
      Object.prototype.toString.call(value) === "[object Symbol]",
    isBigIntObject: (value) =>
      Object.prototype.toString.call(value) === "[object BigInt]",
    isNativeError: (value) =>
      value instanceof Error &&
      Object.prototype.toString.call(value) === "[object Error]",
    isRegExp: (value) => value instanceof RegExp,
    isAsyncFunction: (value) =>
      Object.prototype.toString.call(value) === "[object AsyncFunction]",
    isGeneratorFunction: (value) =>
      Object.prototype.toString.call(value) === "[object GeneratorFunction]",
    isGeneratorObject: (value) =>
      Object.prototype.toString.call(value) === "[object Generator]",
    isMap: (value) => value instanceof Map,
    isSet: (value) => value instanceof Set,
    isWeakMap: (value) => value instanceof WeakMap,
    isWeakSet: (value) => value instanceof WeakSet,
    isArrayBuffer: (value) => value instanceof ArrayBuffer,
    isSharedArrayBuffer: (value) => value instanceof SharedArrayBuffer,
    isAnyArrayBuffer: (value) =>
      value instanceof ArrayBuffer || value instanceof SharedArrayBuffer,
    isArrayBufferView: (value) => ArrayBuffer.isView(value),
    isDataView: (value) => __nodeDataViewSet.has(value),
    isBoxedPrimitive: (value) =>
      value instanceof Boolean ||
      value instanceof Number ||
      value instanceof String ||
      Object.prototype.toString.call(value) === "[object Symbol]" ||
      Object.prototype.toString.call(value) === "[object BigInt]",
    isArgumentsObject: (value) =>
      Object.prototype.toString.call(value) === "[object Arguments]",
    isMapIterator: (value) =>
      Object.prototype.toString.call(value) === "[object Map Iterator]",
    isSetIterator: (value) =>
      Object.prototype.toString.call(value) === "[object Set Iterator]",
    isTypedArray: (value) =>
      ArrayBuffer.isView(value) && !__nodeUtil.types.isDataView(value),
    isUint8Array: (value) =>
      __nodeTypedArraySets.Uint8Array.has(value) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object Uint8Array]"),
    isUint8ClampedArray: (value) =>
      __nodeTypedArraySets.Uint8ClampedArray.has(value) ||
      (ArrayBuffer.isView(value) && value instanceof Uint8ClampedArray) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object Uint8ClampedArray]"),
    isInt8Array: (value) =>
      __nodeTypedArraySets.Int8Array.has(value) ||
      (ArrayBuffer.isView(value) && value instanceof Int8Array) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object Int8Array]"),
    isUint16Array: (value) =>
      __nodeTypedArraySets.Uint16Array.has(value) ||
      (ArrayBuffer.isView(value) && value instanceof Uint16Array) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object Uint16Array]"),
    isInt16Array: (value) =>
      __nodeTypedArraySets.Int16Array.has(value) ||
      (ArrayBuffer.isView(value) && value instanceof Int16Array) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object Int16Array]"),
    isUint32Array: (value) =>
      __nodeTypedArraySets.Uint32Array.has(value) ||
      (ArrayBuffer.isView(value) && value instanceof Uint32Array) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object Uint32Array]"),
    isInt32Array: (value) =>
      __nodeTypedArraySets.Int32Array.has(value) ||
      (ArrayBuffer.isView(value) && value instanceof Int32Array) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object Int32Array]"),
    isFloat32Array: (value) =>
      __nodeTypedArraySets.Float32Array.has(value) ||
      (ArrayBuffer.isView(value) && value instanceof Float32Array) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object Float32Array]"),
    isFloat64Array: (value) =>
      __nodeTypedArraySets.Float64Array.has(value) ||
      (ArrayBuffer.isView(value) && value instanceof Float64Array) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object Float64Array]"),
    isFloat16Array: (value) =>
      __nodeTypedArraySets.Float16Array.has(value) ||
      (typeof Float16Array !== "undefined" &&
        ArrayBuffer.isView(value) &&
        value instanceof Float16Array) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object Float16Array]"),
    isBigInt64Array: (value) =>
      __nodeTypedArraySets.BigInt64Array.has(value) ||
      (ArrayBuffer.isView(value) && value instanceof BigInt64Array) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object BigInt64Array]"),
    isBigUint64Array: (value) =>
      __nodeTypedArraySets.BigUint64Array.has(value) ||
      (ArrayBuffer.isView(value) && value instanceof BigUint64Array) ||
      (ArrayBuffer.isView(value) &&
        Object.prototype.toString.call(value) === "[object BigUint64Array]"),
    isProxy: (value) => __nodeProxySet.has(value),
    isExternal: (value) => value && value.__quench_external === true,
    isModuleNamespaceObject: (value) => __nodeModuleNamespaces.has(value),
    isCryptoKey: (value) =>
      globalThis.__quenchWebCryptoKeyBrand?.has(value) === true,
    isKeyObject: () => false
  }
};
globalThis.__nodeUtil.inspect.defaultOptions = { numericSeparator: false };
globalThis.__nodeUtil.inspect.custom = Symbol.for("nodejs.util.inspect.custom");
let __nodeInspectMaxBytes = 50;
