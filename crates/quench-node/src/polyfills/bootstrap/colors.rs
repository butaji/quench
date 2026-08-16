//! Polyfill: `colors`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeUtilFormatColor = (value) => {
  if (value === null) return "\u001b[1mnull\u001b[22m";
  if (value === undefined) return "\u001b[90mundefined\u001b[39m";
  if (
    typeof value === "boolean" ||
    typeof value === "number" ||
    typeof value === "bigint"
  ) {
    return `\u001b[33m${String(value)}${
      typeof value === "bigint" ? "n" : ""
    }\u001b[39m`;
  }
  if (typeof value === "symbol") return `\u001b[32m${String(value)}\u001b[39m`;
  return String(value);
};
const __nodeUtilFormatCompact = (options, args) => {
  if (
    options.compact === undefined ||
    args[0] !== "%s" ||
    !Array.isArray(args[1])
  ) {
    return null;
  }
  return `[ ${args[1]
    .map((value) =>
      value && typeof value === "object" ? "[Object]" : String(value)
    )
    .join(", ")} ]`;
};
globalThis.__nodeUtil.formatWithOptions = (options, ...args) => {
  if (!options || typeof options !== "object" || Array.isArray(options)) {
    const error = new TypeError(
      'The "inspectOptions" argument must be an object.' +
        globalThis.__nodeCommon.invalidArgTypeHelper(options)
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options.colors && typeof args[0] !== "string") {
    return args.map(__nodeUtilFormatColor).join(" ");
  }
  const compact = __nodeUtilFormatCompact(options, args);
  if (compact !== null) return compact;
  const previous =
    globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator;
  if (options && options.numericSeparator !== undefined) {
    globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator =
      options.numericSeparator;
  }
  try {
    return globalThis.__nodeUtil.format(...args);
  } finally {
    globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator = previous;
  }
};
const __nodeQuerystringEscape = (value) => {
  if (typeof value === "symbol") {
    throw new TypeError("Cannot convert a Symbol value to a string");
  }
  const input = String(value);
  let normalized = "";
  for (let index = 0; index < input.length; index += 1) {
    const code = input.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      if (index + 1 === input.length) {
        throw Object.assign(new URIError("URI malformed"), { code: "ERR_INVALID_URI" });
      }
      const next = input.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        normalized += String.fromCodePoint(
          0x10000 + ((code - 0xd800) << 10) + next - 0xdc00
        );
      } else {
        normalized += String.fromCodePoint(
          0x10000 + ((code - 0xd800) << 10) + next
        );
      }
      index += 1;
      continue;
    }
    normalized += input[index];
  }
  try {
    return encodeURIComponent(normalized);
  } catch (_) {
    throw Object.assign(new URIError("URI malformed"), { code: "ERR_INVALID_URI" });
  }
};
const __nodeQuerystringDecodeByte = (input, index, decodeSpaces) => {
  if (input[index] === "+" && decodeSpaces) {
    return { bytes: [0x20], advance: 0 };
  }
  if (
    input[index] === "%" &&
    /^[0-9a-f]{2}$/i.test(input.slice(index + 1, index + 3))
  ) {
    return {
      bytes: [parseInt(input.slice(index + 1, index + 3), 16)],
      advance: 2
    };
  }
  let character = input[index];
  if (
    input.charCodeAt(index) >= 0xd800 &&
    input.charCodeAt(index) <= 0xdbff &&
    index + 1 < input.length &&
    input.charCodeAt(index + 1) >= 0xdc00 &&
    input.charCodeAt(index + 1) <= 0xdfff
  ) {
    character += input[index + 1];
  }
  return {
    bytes: Array.from(new TextEncoder().encode(character)),
    advance: character.length - 1
  };
};
const __nodeQuerystringParseValue = (value, options) => {
  const inputValue = String(value).replace(/\+/g, " ");
  if (options && options.decodeURIComponent) {
    try {
      return options.decodeURIComponent(inputValue);
    } catch (_) {}
  }
  return globalThis.__nodeQuerystring.unescape(inputValue);
};
const __nodeQuerystringAddValue = (result, key, value) => {
  result[key] =
    result[key] === undefined
      ? value
      : Array.isArray(result[key])
        ? result[key].concat(value)
        : [result[key], value];
};
const __nodeQuerystringMaxKeys = (options) => {
  if (!options || options.maxKeys === undefined) return 1000;
  if (typeof options.maxKeys !== "number") {
    return options.maxKeys === "0" ? Infinity : 1000;
  }
  if (!Number.isFinite(options.maxKeys)) return Infinity;
  return options.maxKeys === 0 ? Infinity : options.maxKeys;
};
const __nodeQuerystringExports = {
  escape: __nodeQuerystringEscape,
  unescape: (value) =>
    globalThis.__nodeQuerystring.unescapeBuffer(String(value), true).toString(),
  unescapeBuffer: (value, decodeSpaces = false) => {
    const input = String(value);
    const bytes = [];
    for (let i = 0; i < input.length; i++) {
      const decoded = __nodeQuerystringDecodeByte(input, i, decodeSpaces);
      bytes.push(...decoded.bytes);
      i += decoded.advance;
    }
    return NodeBuffer.from(new Uint8Array(bytes));
  },
  stringify: (object, sep = "&", eq = "=", options = {}) => {
    if (!object || typeof object !== "object") return "";
    sep = sep == null ? "&" : String(sep);
    eq = eq == null ? "=" : String(eq);
    const encode =
      options && options.encodeURIComponent
        ? options.encodeURIComponent
        : globalThis.__nodeQuerystring.escape;
    return Object.keys(object)
      .flatMap((key) => {
        const value = object[key];
        if (Array.isArray(value) && value.length === 0) return [];
        const values = Array.isArray(value) ? value : [value];
        return values.map((item) => {
          let encodedKey;
          let encodedValue;
          try {
            encodedKey = encode(key);
            encodedValue =
              item === null ||
              typeof item === "object" ||
              typeof item === "function" ||
              (typeof item === "number" && !Number.isFinite(item))
                ? ""
                : encode(item);
          } catch (error) {
            if (error instanceof URIError) {
              error.code = "ERR_INVALID_URI";
              error.message = "URI malformed";
            }
            throw error;
          }
          return `${encodedKey}${eq}${encodedValue}`;
        });
      })
      .join(sep);
  },
  parse: (input, sep = "&", eq = "=", options = {}) => {
    const result = Object.create(null);
    if (input == null || input === "") return result;
    const separator = sep == null ? "&" : String(sep);
    const equals = eq == null ? "=" : String(eq);
    const maxKeys = __nodeQuerystringMaxKeys(options);
    String(input)
      .split(separator)
      .slice(0, maxKeys)
      .filter(Boolean)
      .forEach((part) => {
        const index = part.indexOf(equals);
        const key = __nodeQuerystringParseValue(
          index < 0 ? part : part.slice(0, index),
          options
        );
        const value = __nodeQuerystringParseValue(
          index < 0 ? "" : part.slice(index + equals.length),
          options
        );
        __nodeQuerystringAddValue(result, key, value);
      });
    return result;
  }
};
let __nodeQuerystringInstance;
globalThis.__nodeQuerystringInitialized = false;
globalThis.__nodeUrlInitialized = false;
globalThis.__nodeQuerystring = __nodeQuerystringExports;
globalThis.__nodeQuerystringInitialized = true;
class NodeURLSearchParams {
  constructor(init = "") {
    this._pairs = [];
    if (typeof init === "string") {
      init
        .replace(/^\?/, "")
        .split("&")
        .filter(Boolean)
        .forEach((part) => {
          const i = part.indexOf("=");
          this.append(
            globalThis.__nodeURLDecode(
              (i < 0 ? part : part.slice(0, i)).replace(/\+/g, " ")
            ),
            globalThis.__nodeURLDecode(
              (i < 0 ? "" : part.slice(i + 1)).replace(/\+/g, " ")
            )
          );
        });
    } else Object.keys(init).forEach((key) => this.append(key, init[key]));
  }
  append(key, value) {
    this._pairs.push([
      globalThis
        .__nodeURLSearchString(key)
        .replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/g, "\uFFFD")
        .replace(/(^|[^\uD800-\uDBFF])[\uDC00-\uDFFF]/g, "$1\uFFFD"),
      globalThis
        .__nodeURLSearchString(value)
        .replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/g, "\uFFFD")
        .replace(/(^|[^\uD800-\uDBFF])[\uDC00-\uDFFF]/g, "$1\uFFFD")
    ]);
  }
  set(key, value) {
    this.delete(key);
    this.append(key, value);
  }
  get(key) {
    const pair = this._pairs.find(
      ([name]) => name === globalThis.__nodeURLSearchString(key)
    );
    return pair ? pair[1] : null;
  }
  getAll(key) {
    return this._pairs
      .filter(([name]) => name === globalThis.__nodeURLSearchString(key))
      .map(([, value]) => value);
  }
  has(key) {
    return this._pairs.some(
      ([name]) => name === globalThis.__nodeURLSearchString(key)
    );
  }
  delete(key) {
    this._pairs = this._pairs.filter(
      ([name]) => name !== globalThis.__nodeURLSearchString(key)
    );
  }
  toString() {
    return this._pairs
      .map(
        ([key, value]) =>
          `${globalThis.__nodeURLFormEncode(key).replace(/%20/g, "+")}=${globalThis
            .__nodeURLFormEncode(value)
            .replace(/%20/g, "+")}`
      )
      .join("&");
  }
}
globalThis.__nodeURLSearchParams = NodeURLSearchParams;
// eslint-disable-next-line complexity
"#);
