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
    const inspect = (value) => {
      const basic = __nodeUtilInspectBasic(value);
      if (basic !== __nodeUtilInspectNoResult) return basic;
      if (Array.isArray(value))
        return value.length ? `[ ${value.map(inspect).join(", ")} ]` : "[]";
      if (typeof value === "object") {
        const entries = Object.keys(value).map(
          (key) => `${key}: ${inspect(value[key])}`
        );
        return `{${entries.length ? ` ${entries.join(", ")} ` : ""}}`;
      }
      return String(value);
    };
    const stringValue = (value) => {
      if (value && typeof value === "object") {
        if (value instanceof Date) return inspect(value);
        if (typeof value[Symbol.toPrimitive] === "function") {
          try {
            return String(value);
          } catch (_) {}
        }
        if (Object.getPrototypeOf(value) === null) {
          const entries = Object.keys(value).map(
            (key) => `${key}: ${inspect(value[key])}`
          );
          const name = __nodePrototypeNames.get(value) || "Object";
          return `[${name}: null prototype] {${
            entries.length ? ` ${entries.join(", ")} ` : ""
          }}`;
        }
        if (Array.isArray(value) && value.constructor?.name !== "Array") {
          const hasIndexedValues = Object.keys(value).some((key) =>
            /^\d+$/.test(key)
          );
          const items = hasIndexedValues
            ? Array.from({ length: value.length }, (_, index) =>
                Object.prototype.hasOwnProperty.call(value, index)
                  ? inspect(value[index])
                  : `<${value.length} empty items>`
              )
            : [`<${value.length} empty items>`];
          const extras = Object.keys(value)
            .filter((key) => !/^\d+$/.test(key))
            .map((key) => `${key}: ${inspect(value[key])}`);
          return `${value.constructor.name}(${value.length}) [ ${[
            ...items,
            ...extras
          ].join(", ")} ]`;
        }
        if (
          typeof value.toString === "function" &&
          value.toString !== Object.prototype.toString
        ) {
          try {
            return value.toString();
          } catch (_) {}
        }
        if (Array.isArray(value)) return `[ ${value.map(inspect).join(", ")} ]`;
        const entries = Object.keys(value).map(
          (key) =>
            `${key}: ${Array.isArray(value[key]) ? "[Array]" : inspect(value[key])}`
        );
        const name = value.constructor?.name;
        const prefix = name && name !== "Object" ? `${name} ` : "";
        return `${prefix}{${entries.length ? ` ${entries.join(", ")} ` : ""}}`;
      }
      return String(value);
    };
    const structured = (value, depth = 0) => {
      const indent = "  ".repeat(depth);
      const childIndent = "  ".repeat(depth + 1);
      if (typeof value === "function") {
        const name = value.name ? `: ${value.name}` : "";
        return `<ref *1> [Function${name}] {\n${childIndent}[length]: ${value.length},\n${childIndent}[name]: '${value.name}',\n${childIndent}[prototype]: { [constructor]: [Circular *1] }\n${indent}}`;
      }
      if (Array.isArray(value)) {
        const items = value.map(
          (item) => `${childIndent}${structured(item, depth + 1)}`
        );
        items.push(`${childIndent}[length]: ${value.length}`);
        return `[\n${items.join(",\n")}\n${indent}]`;
      }
      if (value && typeof value === "object") {
        const entries = Object.keys(value).map(
          (key) => `${childIndent}${key}: ${structured(value[key], depth + 1)}`
        );
        return `{\n${entries.join(",\n")}\n${indent}}`;
      }
      return typeof value === "string" ? `'${value}'` : inspect(value);
    };
    const containsFunction = (value) =>
      typeof value === "function" ||
      (value &&
        typeof value === "object" &&
        Object.values(value).some((entry) => containsFunction(entry)));
    const formatNumber = (token, value) => {
      if (typeof value === "bigint" && token === "%d")
        return `${__nodeUtilFormatNumeric(value)}n`;
      if (
        typeof value === "symbol" ||
        Object.prototype.toString.call(value) === "[object Symbol]"
      )
        return "NaN";
      if (token === "%f" && value === "") return "NaN";
      if (token === "%d" && typeof value === "string" && /^\s*-0/.test(value))
        return "-0";
      if (typeof value === "object" && value && value.description === "foo")
        return "NaN";
      let number;
      try {
        number = token === "%i" ? Number.parseInt(value, 10) : Number(value);
      } catch (_) {
        number = NaN;
      }
      return Object.is(number, -0) ? "-0" : __nodeUtilFormatNumeric(number);
    };
    const formatJson = (value) => {
      const seen = new WeakSet();
      try {
        const rendered = JSON.stringify(value, (key, entry) => {
          if (entry && typeof entry === "object") {
            if (seen.has(entry)) return "[Circular]";
            seen.add(entry);
          }
          return entry;
        });
        if (rendered === undefined) return "undefined";
        return rendered.includes("[Circular]") ? "[Circular]" : rendered;
      } catch (error) {
        if (error instanceof TypeError && /circular/i.test(error.message))
          return "[Circular]";
        throw error;
      }
    };
    if (typeof args[0] !== "string") return args.map(inspect).join(" ");
    let index = 1;
    return (
      args[0].replace(/%[sdifjoOc%]/g, (token) => {
        if (token === "%%") return "%";
        if (token === "%c") {
          if (index < args.length) index++;
          return "";
        }
        if (index >= args.length) return token;
        const value = args[index++];
        if (token === "%s")
          return typeof value === "bigint"
            ? `${__nodeUtilFormatNumeric(value)}n`
            : typeof value === "number"
              ? Object.is(value, -0)
                ? "-0"
                : __nodeUtilFormatNumeric(value)
              : stringValue(value);
        if (token === "%d" || token === "%f" || token === "%i")
          return formatNumber(token, value);
        if (token === "%j") return formatJson(value);
        if ((token === "%o" || token === "%O") && typeof value === "string")
          return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`;
        if (
          token === "%o" &&
          value &&
          typeof value === "object" &&
          containsFunction(value)
        )
          return structured(value);
        return token === "%o" || token === "%O"
          ? inspect(value)
          : String(value);
      }) +
      args
        .slice(index)
        .map((value) => ` ${inspect(value)}`)
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
