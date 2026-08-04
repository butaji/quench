const __nodeUtilInspectBuffer = (value) => {
  const custom = value[Symbol.for("nodejs.util.inspect.custom")];
  const properties = Object.keys(value)
    .filter((key) => !/^\d+$/.test(key))
    .map((key) => {
      const item = value[key];
      if (item instanceof Uint8Array)
        return `${key}: ${item.constructor.name}(${item.length}) []`;
      return `${key}: ${item === undefined ? "undefined" : String(item)}`;
    });
  const rendered =
    typeof custom === "function"
      ? custom.call(value)
      : `<Buffer ${Array.from(value).join(" ")}>`;
  if (typeof custom === "function" && properties.length)
    return `${rendered.slice(0, -1)}${rendered.endsWith("Buffer >") ? "" : ", "}${properties.join(", ")}>`;
  return rendered;
};
const __nodeUtilFormatNumeric = (value) => {
  const rendered = String(value);
  if (!globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator)
    return rendered;
  const [mantissa, exponent] = rendered.split("e");
  const sign = mantissa.startsWith("-") ? "-" : "";
  const unsigned = sign ? mantissa.slice(1) : mantissa;
  const [whole, fraction] = unsigned.split(".");
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, "_");
  return `${sign}${grouped}${fraction === undefined ? "" : `.${fraction}`}${exponent === undefined ? "" : `e${exponent}`}`;
};
const __nodeUtilInspectNoResult = Symbol("inspect-no-result");
const __nodeUtilInspectSharedBuffer = (value) =>
  `SharedArrayBuffer { [Uint8Contents]: <${Array.from(new Uint8Array(value), (byte) => byte.toString(16).padStart(2, "0")).join(" ")}>, [byteLength]: ${value.byteLength} }`;
const __nodeUtilInspectError = (value) =>
  Object.prototype.hasOwnProperty.call(value, "stack")
    ? value.stack || `${value.name}: ${value.message}`
    : `[${value.name}: ${value.message}]`;
const __nodeUtilInspectFunction = (value) =>
  `[Function${value.name ? `: ${value.name}` : " (anonymous)"}]`;
const __nodeUtilInspectBasic = (value) => {
  if (value === null) return "null";
  if (value === undefined) return "undefined";
  if (
    typeof SharedArrayBuffer !== "undefined" &&
    value instanceof SharedArrayBuffer
  )
    return __nodeUtilInspectSharedBuffer(value);
  if (value instanceof Date) return value.toISOString();
  if (value instanceof Error) return __nodeUtilInspectError(value);
  if (typeof value === "string") return value;
  if (typeof value === "symbol") return String(value);
  if (typeof value === "function") return __nodeUtilInspectFunction(value);
  return __nodeUtilInspectNoResult;
};
const __nodeUtilSpecialNumber = (token, value) =>
  typeof value === "symbol" ||
  Object.prototype.toString.call(value) === "[object Symbol]" ||
  (token === "%f" && value === "") ||
  (typeof value === "object" && value && value.description === "foo");
const __nodeUtilFormatNumber = (token, value) => {
  if (typeof value === "bigint" && (token === "%d" || token === "%i"))
    return `${__nodeUtilFormatNumeric(value)}n`;
  if (__nodeUtilSpecialNumber(token, value)) return "NaN";
  let number;
  try {
    number = token === "%i" ? Number.parseInt(value, 10) : Number(value);
  } catch (_) {
    number = NaN;
  }
  return Object.is(number, -0) ? "-0" : __nodeUtilFormatNumeric(number);
};
const __nodeUtilCircularReplacer = (seen) => (key, entry) => {
  if (entry && typeof entry === "object") {
    if (seen.has(entry)) return "[Circular]";
    seen.add(entry);
  }
  return entry;
};
const __nodeUtilFormatJson = (value) => {
  const seen = new WeakSet();
  try {
    const rendered = JSON.stringify(value, __nodeUtilCircularReplacer(seen));
    if (rendered === undefined) return "undefined";
    return rendered.includes("[Circular]") ? "[Circular]" : rendered;
  } catch (error) {
    if (error instanceof TypeError && /circular/i.test(error.message))
      return "[Circular]";
    throw error;
  }
};
const __nodeUtilInspectValue = (value, quoteStrings = false) => {
  if (quoteStrings && typeof value === "string")
    return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`;
  const basic = __nodeUtilInspectBasic(value);
  if (basic !== __nodeUtilInspectNoResult) return basic;
  if (Array.isArray(value))
    return value.length
      ? `[ ${value.map((item) => __nodeUtilInspectValue(item, quoteStrings)).join(", ")} ]`
      : "[]";
  if (typeof value === "object") {
    const entries = Object.keys(value).map(
      (key) => `${key}: ${__nodeUtilInspectValue(value[key], quoteStrings)}`
    );
    return `{${entries.length ? ` ${entries.join(", ")} ` : ""}}`;
  }
  return String(value);
};
const __nodeUtilInspectNamedArray = (value) => {
  const indexed = Object.keys(value).some((key) => /^\d+$/.test(key));
  const items = indexed
    ? Array.from({ length: value.length }, (_, index) =>
        Object.prototype.hasOwnProperty.call(value, index)
          ? __nodeUtilInspectValue(value[index])
          : `<${value.length} empty items>`
      )
    : [`<${value.length} empty items>`];
  const extras = Object.keys(value)
    .filter((key) => !/^\d+$/.test(key))
    .map((key) => `${key}: ${__nodeUtilInspectValue(value[key])}`);
  return `${value.constructor.name}(${value.length}) [ ${[
    ...items,
    ...extras
  ].join(", ")} ]`;
};
const __nodeUtilContainsFunction = (value) =>
  typeof value === "function" ||
  (value &&
    typeof value === "object" &&
    Object.values(value).some((entry) => __nodeUtilContainsFunction(entry)));
const __nodeUtilStructured = (value, depth = 0) => {
  const indent = "  ".repeat(depth);
  const childIndent = "  ".repeat(depth + 1);
  if (typeof value === "function") {
    const name = value.name ? `: ${value.name}` : "";
    return `<ref *1> [Function${name}] {\n${childIndent}[length]: ${value.length},\n${childIndent}[name]: '${value.name}',\n${childIndent}[prototype]: { [constructor]: [Circular *1] }\n${indent}}`;
  }
  if (Array.isArray(value)) {
    const items = value.map(
      (item) => `${childIndent}${__nodeUtilStructured(item, depth + 1)}`
    );
    items.push(`${childIndent}[length]: ${value.length}`);
    return `[\n${items.join(",\n")}\n${indent}]`;
  }
  if (value && typeof value === "object") {
    const entries = Object.keys(value).map(
      (key) =>
        `${childIndent}${key}: ${__nodeUtilStructured(value[key], depth + 1)}`
    );
    return `{\n${entries.join(",\n")}\n${indent}}`;
  }
  return typeof value === "string"
    ? `'${value}'`
    : __nodeUtilInspectValue(value);
};
const __nodeUtilFormatNullPrototype = (value) => {
  const entries = Object.keys(value).map(
    (key) => `${key}: ${__nodeUtilInspectValue(value[key])}`
  );
  const name = __nodePrototypeNames.get(value) || "Object";
  return `[${name}: null prototype] {${entries.length ? ` ${entries.join(", ")} ` : ""}}`;
};
const __nodeUtilFormatCustomString = (value) => {
  if (typeof value.toString !== "function") return null;
  if (value.toString === Object.prototype.toString) return null;
  try {
    return value.toString();
  } catch (_) {
    return null;
  }
};
const __nodeUtilFormatPrimitiveObject = (value) => {
  if (typeof value[Symbol.toPrimitive] !== "function") return null;
  try {
    return String(value);
  } catch (_) {
    return null;
  }
};
const __nodeUtilFormatArrayValue = (value) =>
  `[ ${value.map(__nodeUtilInspectValue).join(", ")} ]`;
const __nodeUtilIsNamedArray = (value) =>
  Array.isArray(value) && value.constructor?.name !== "Array";
const __nodeUtilFormatObjectValue = (value) => {
  const entries = Object.keys(value).map(
    (key) =>
      `${key}: ${Array.isArray(value[key]) ? "[Array]" : __nodeUtilInspectValue(value[key])}`
  );
  const name = value.constructor?.name;
  const prefix = name && name !== "Object" ? `${name} ` : "";
  return `${prefix}{${entries.length ? ` ${entries.join(", ")} ` : ""}}`;
};
const __nodeUtilFormatStringValue = (value) => {
  if (!value || typeof value !== "object") return String(value);
  if (value instanceof Date) return __nodeUtilInspectValue(value);
  const primitive = __nodeUtilFormatPrimitiveObject(value);
  if (primitive !== null) return primitive;
  if (Object.getPrototypeOf(value) === null)
    return __nodeUtilFormatNullPrototype(value);
  if (__nodeUtilIsNamedArray(value)) return __nodeUtilInspectNamedArray(value);
  const custom = __nodeUtilFormatCustomString(value);
  if (custom !== null) return custom;
  if (Array.isArray(value)) return __nodeUtilFormatArrayValue(value);
  return __nodeUtilFormatObjectValue(value);
};
const __nodeUtilFormatStringToken = (value) => {
  if (typeof value === "bigint") return `${__nodeUtilFormatNumeric(value)}n`;
  if (typeof value === "number")
    return Object.is(value, -0) ? "-0" : __nodeUtilFormatNumeric(value);
  return __nodeUtilFormatStringValue(value);
};
const __nodeUtilFormatObjectToken = (token, value) => {
  if ((token === "%o" || token === "%O") && typeof value === "string")
    return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`;
  if (
    token === "%o" &&
    value &&
    typeof value === "object" &&
    __nodeUtilContainsFunction(value)
  )
    return __nodeUtilStructured(value);
  return token === "%o" || token === "%O"
    ? __nodeUtilInspectValue(value, token === "%O")
    : String(value);
};
const __nodeUtilFormatTokenValue = (token, value) => {
  if (token === "%s") return __nodeUtilFormatStringToken(value);
  if (token === "%d" || token === "%f" || token === "%i")
    return __nodeUtilFormatNumber(token, value);
  if (token === "%j") return __nodeUtilFormatJson(value);
  return __nodeUtilFormatObjectToken(token, value);
};
globalThis.__nodeUtil = {
  TextEncoder: globalThis.TextEncoder,
  TextDecoder: globalThis.TextDecoder,
  isArray: (value) => Array.isArray(value),
  debuglog: () => () => {},
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
      const error = new TypeError(
        'The "str" argument must be of type string.' +
          ` Received type ${typeof value} (${String(value)})`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return value.replace(
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
    if (typeof args[0] !== "string")
      return args.map(__nodeUtilInspectValue).join(" ");
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
  inspect: (value) => {
    if (value instanceof Date) return value.toISOString();
    if (typeof value === "symbol") return String(value);
    if (typeof value === "string")
      return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`;
    if (typeof value === "function")
      return value.name
        ? `[${value.constructor.name}: ${value.name}]`
        : `[${value.constructor.name} (anonymous)]`;
    if (value instanceof NodeBuffer) return __nodeUtilInspectBuffer(value);
    if (
      value &&
      typeof value[Symbol.for("nodejs.util.inspect.custom")] === "function"
    )
      return value[Symbol.for("nodejs.util.inspect.custom")]();
    try {
      return JSON.stringify(value);
    } catch (_) {
      return String(value);
    }
  },
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
    isCryptoKey: () => false,
    isKeyObject: () => false
  }
};
globalThis.__nodeUtil.inspect.defaultOptions = { numericSeparator: false };
let __nodeInspectMaxBytes = 50;
