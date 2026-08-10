//! Polyfill: `formatting-tail`

pub const JS: &str = r#"const __nodeUtilInspectBuffer = (value) => {
  const custom = value[Symbol.for("nodejs.util.inspect.custom")];
  const properties = Object.keys(value)
    .filter((key) => !/^\d+$/.test(key))
    .map((key) => {
      const item = value[key];
      if (item instanceof Uint8Array) {
        return `${key}: ${item.constructor.name}(${item.length}) []`;
      }
      return `${key}: ${item === undefined ? "undefined" : String(item)}`;
    });
  const rendered =
    typeof custom === "function"
      ? custom.call(value)
      : `<Buffer ${Array.from(value).join(" ")}>`;
  if (typeof custom === "function" && properties.length) {
    return `${rendered.slice(0, -1)}${
      rendered.endsWith("Buffer >") ? "" : ", "
    }${properties.join(", ")}>`;
  }
  return rendered;
};
const __nodeUtilFormatNumeric = (value) => {
  const rendered = String(value);
  if (!globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator) {
    return rendered;
  }
  const [mantissa, exponent] = rendered.split("e");
  const sign = mantissa.startsWith("-") ? "-" : "";
  const unsigned = sign ? mantissa.slice(1) : mantissa;
  const [whole, fraction] = unsigned.split(".");
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, "_");
  return `${sign}${grouped}${fraction === undefined ? "" : `.${fraction}`}${
    exponent === undefined ? "" : `e${exponent}`
  }`;
};
const __nodeUtilInspectNoResult = Symbol("inspect-no-result");
const __nodeUtilInspectSharedBuffer = (value) =>
  `SharedArrayBuffer { [Uint8Contents]: <${Array.from(
    new Uint8Array(value),
    (byte) => byte.toString(16).padStart(2, "0")
  ).join(" ")}>, [byteLength]: ${value.byteLength} }`;
const __nodeUtilInspectError = (value) => {
  let base = Object.prototype.hasOwnProperty.call(value, "stack")
    ? value.stack || `${value.name}: ${value.message}`
    : `[${value.name}: ${value.message}]`;
  if (typeof base === "string") {
    const firstNewline = base.indexOf("\n");
    const firstLine = firstNewline < 0 ? base : base.slice(0, firstNewline);
    if (firstLine.length > 9500) {
      const suffix = firstNewline < 0 ? "" : base.slice(firstNewline);
      base = `${firstLine.slice(0, 9488)}...${suffix}`;
    }
  }
  const keys = "generatedMessage code actual expected operator diff"
    .split(" ")
    .filter((key) => Object.prototype.hasOwnProperty.call(value, key));
  const inspectField = (key) => {
    const field = value[key];
    if (typeof field === "string") {
      const escaped = field.replace(/\\/g, "\\\\").replace(/\n/g, "\\n");
      if (escaped.includes("\\n") && escaped.length > 30) {
        return `'${escaped.slice(0, 30)}...'`;
      }
      if (escaped.length > 9488) return `'${escaped.slice(0, 9488)}...'`;
    }
    return __nodeUtilInspectValue(field, true);
  };
  return keys.length
    ? `${base} { ${keys
        .map((key) => `${key}: ${inspectField(key)}`)
        .join(", ")} }`
    : base;
};
const __nodeUtilInspectFunction = (value) => {
  const source = Function.prototype.toString.call(value);
  const tag = Object.prototype.toString.call(value).slice(8, -1);
  const generator =
    /^\s*(?:async\s+)?function\*/.test(source) ||
    tag.includes("GeneratorFunction");
  if (!generator) {
    const kind =
      /^\s*async\s+function/.test(source) || tag === "AsyncFunction"
        ? "AsyncFunction"
        : "Function";
    return `[${kind}${value.name ? `: ${value.name}` : " (anonymous)"}]`;
  }
  const asyncGenerator =
    /^\s*async\s+function\*/.test(source) || tag === "AsyncGeneratorFunction";
  const kind = asyncGenerator ? "AsyncGeneratorFunction" : "GeneratorFunction";
  const nullPrototype =
    Object.getPrototypeOf(value) === null ? " (null prototype)" : "";
  const customTag =
    value[Symbol.toStringTag] &&
    !/^(?:Async)?(?:Generator)?Function$/.test(value[Symbol.toStringTag])
      ? ` [${value[Symbol.toStringTag]}]`
      : "";
  const functionConstructor =
    value.constructor?.name || Object.getPrototypeOf(value)?.constructor?.name;
  const asyncSuffix =
    !asyncGenerator &&
    (functionConstructor === "AsyncFunction" || (customTag && !nullPrototype))
      ? " AsyncFunction"
      : "";
  return `[${kind}${nullPrototype}${
    value.name ? `: ${value.name}` : " (anonymous)"
  }]${asyncSuffix}${customTag}`;
};
const __nodeUtilInspectBasic = (value) => {
  if (value === null) return "null";
  if (value === undefined) return "undefined";
  if (
    typeof SharedArrayBuffer !== "undefined" &&
    value instanceof SharedArrayBuffer
  ) {
    return __nodeUtilInspectSharedBuffer(value);
  }
  if (value instanceof Date) return value.toISOString();
  if (value instanceof Error) return __nodeUtilInspectError(value);
  if (typeof value === "string") return value;
  if (typeof value === "symbol") return String(value);
  if (typeof value === "function") return __nodeUtilInspectFunction(value);
  return __nodeUtilInspectNoResult;
};
const __nodeUtilInspectPrimitive = (value, quoteStrings) => {
  if (quoteStrings && typeof value === "string") {
    return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`;
  }
  if (Object.is(value, -0)) return "-0";
  if (typeof value === "bigint") return `${value}n`;
  return undefined;
};
const __nodeUtilSpecialNumber = (token, value) =>
  typeof value === "symbol" ||
  Object.prototype.toString.call(value) === "[object Symbol]" ||
  (token === "%f" && value === "") ||
  (typeof value === "object" && value && value.description === "foo");
const __nodeUtilFormatNumber = (token, value) => {
  if (typeof value === "bigint" && (token === "%d" || token === "%i")) {
    return `${__nodeUtilFormatNumeric(value)}n`;
  }
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
    if (error instanceof TypeError && /circular/i.test(error.message)) {
      return "[Circular]";
    }
    throw error;
  }
};
const __nodeUtilInspectValue = (value, quoteStrings = false) => {
  const primitive = __nodeUtilInspectPrimitive(value, quoteStrings);
  if (primitive !== undefined) return primitive;
  const basic = __nodeUtilInspectBasic(value);
  if (basic !== __nodeUtilInspectNoResult) return basic;
  if (Array.isArray(value)) {
    return value.length
      ? `[ ${value
          .map((item) => __nodeUtilInspectValue(item, quoteStrings))
          .join(", ")} ]`
      : "[]";
  }
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
  return `[${name}: null prototype] {${
    entries.length ? ` ${entries.join(", ")} ` : ""
  }}`;
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
      `${key}: ${
        Array.isArray(value[key])
          ? "[Array]"
          : __nodeUtilInspectValue(value[key])
      }`
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
  if (Object.getPrototypeOf(value) === null) {
    return __nodeUtilFormatNullPrototype(value);
  }
  if (__nodeUtilIsNamedArray(value)) return __nodeUtilInspectNamedArray(value);
  const custom = __nodeUtilFormatCustomString(value);
  if (custom !== null) return custom;
  if (Array.isArray(value)) return __nodeUtilFormatArrayValue(value);
  return __nodeUtilFormatObjectValue(value);
};
const __nodeUtilFormatStringToken = (value) => {
  if (typeof value === "bigint") return `${__nodeUtilFormatNumeric(value)}n`;
  if (typeof value === "number") {
    return Object.is(value, -0) ? "-0" : __nodeUtilFormatNumeric(value);
  }
  return __nodeUtilFormatStringValue(value);
};
const __nodeUtilFormatObjectToken = (token, value) => {
  if ((token === "%o" || token === "%O") && typeof value === "string") {
    return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`;
  }
  if (
    token === "%o" &&
    value &&
    typeof value === "object" &&
    __nodeUtilContainsFunction(value)
  ) {
    return __nodeUtilStructured(value);
  }
  return token === "%o" || token === "%O"
    ? __nodeUtilInspectValue(value, true)
    : String(value);
};
const __nodeUtilFormatTokenValue = (token, value) => {
  if (token === "%s") return __nodeUtilFormatStringToken(value);
  if (token === "%d" || token === "%f" || token === "%i") {
    return __nodeUtilFormatNumber(token, value);
  }
  if (token === "%j") return __nodeUtilFormatJson(value);
  return __nodeUtilFormatObjectToken(token, value);
};
const __nodeUtilDeprecationCodes = new Set();
const __nodeUtilDeprecate = (
  functionToWrap,
  message = "",
  code,
  options = {}
) => {
  if (code !== undefined && typeof code !== "string") {
    const received =
      code === null
        ? "Received null"
        : `Received type ${typeof code} (${String(code)})`;
    throw Object.assign(new TypeError('The "code" argument must be of type string. ' + received), { code: "ERR_INVALID_ARG_TYPE" });
  }
  const warningKey = code === undefined ? functionToWrap : code;
  const deprecatedFunction = function (...args) {
    if (!__nodeUtilDeprecationCodes.has(warningKey)) {
      __nodeUtilDeprecationCodes.add(warningKey);
      setImmediate(() =>
        process.emitWarning(String(message), {
          name: "DeprecationWarning",
          code
        })
      );
    }
    return functionToWrap.apply(this, args);
  };
  Object.defineProperty(deprecatedFunction, "length", {
    configurable: true,
    value: functionToWrap.length
  });
  if (options.modifyPrototype !== false) {
    deprecatedFunction.prototype = functionToWrap.prototype;
    Object.setPrototypeOf(deprecatedFunction, functionToWrap);
  }
  return deprecatedFunction;
};
const __nodeUtilPendingDeprecate = (functionToWrap, message, code) =>
  __nodeUtilDeprecate(functionToWrap, message, code);
"#;
