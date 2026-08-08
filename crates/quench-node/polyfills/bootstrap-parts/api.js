globalThis.Buffer = new Proxy(NodeBuffer, {
  apply(_target, _thisArg, args) {
    if (typeof args[0] === "number") {
      return new NodeBuffer(NodeBuffer._validateSize(args[0]));
    }
    return NodeBuffer.from(...args);
  },
  construct(_target, args) {
    if (typeof args[0] === "number") {
      if (typeof args[1] === "string") {
        const error = new TypeError(
          `The "string" argument must be of type string. Received type number (${
            args[0]
          })`
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const buffer = new NodeBuffer(NodeBuffer._validateSize(args[0]));
      Object.defineProperties(buffer, {
        parent: { value: buffer.buffer, configurable: true },
        offset: { value: buffer.byteOffset, configurable: true }
      });
      return buffer;
    }
    return NodeBuffer.from(...args);
  }
});
Object.defineProperties(NodeBuffer.prototype, {
  parent: { value: undefined, configurable: true },
  offset: { value: undefined, configurable: true }
});
NodeBuffer.poolSize = 8192;
for (const name of [
  "ascii",
  "base64",
  "base64url",
  "latin1",
  "hex",
  "ucs2",
  "utf8"
]) {
  NodeBuffer.prototype[`${name}Slice`] = NodeBuffer.prototype.slice;
  NodeBuffer.prototype[`${name}Write`] = NodeBuffer.prototype.write;
}
NodeBuffer.prototype[Symbol.for("nodejs.util.inspect.custom")] =
  NodeBuffer.prototype.inspect;
for (const name of ["8", "16LE", "16BE", "32LE", "32BE"]) {
  NodeBuffer.prototype[`readUint${name}`] =
    NodeBuffer.prototype[`readUInt${name}`];
  NodeBuffer.prototype[`writeUint${name}`] =
    NodeBuffer.prototype[`writeUInt${name}`];
}
NodeBuffer.prototype.readUintLE = NodeBuffer.prototype.readUIntLE;
NodeBuffer.prototype.toLocaleString = NodeBuffer.prototype.toString;
NodeBuffer.prototype.readUintBE = NodeBuffer.prototype.readUIntBE;
NodeBuffer.prototype.writeUintLE = NodeBuffer.prototype.writeUIntLE;
NodeBuffer.prototype.writeUintBE = NodeBuffer.prototype.writeUIntBE;
NodeBuffer.prototype.readBigUint64LE = NodeBuffer.prototype.readBigUInt64LE;
NodeBuffer.prototype.readBigUint64BE = NodeBuffer.prototype.readBigUInt64BE;
NodeBuffer.prototype.writeBigUint64LE = NodeBuffer.prototype.writeBigUInt64LE;
NodeBuffer.prototype.writeBigUint64BE = NodeBuffer.prototype.writeBigUInt64BE;
const __nodeGetOwnPropertyNames = Object.getOwnPropertyNames;
Object.getOwnPropertyNames = (value) => {
  if (value !== NodeBuffer.prototype) return __nodeGetOwnPropertyNames(value);
  const names = new Set();
  for (
    let prototype = value;
    prototype && prototype !== Uint8Array.prototype;
    prototype = Object.getPrototypeOf(prototype)
  ) {
    for (const name of __nodeGetOwnPropertyNames(prototype)) {
      if (
        !name.startsWith("_") &&
        typeof Object.getOwnPropertyDescriptor(prototype, name)?.value ===
          "function"
      ) {
        names.add(name);
      }
    }
  }
  return Array.from(names);
};
const __nodeInvalidCharacter = () => {
  const error = new DOMException(
    "The string contains invalid characters.",
    "InvalidCharacterError"
  );
  error.code = 5;
  return error;
};
function nodeAtob(value) {
  if (arguments.length === 0 || typeof value === "symbol") {
    throw new TypeError("The data is not a string");
  }
  const input = String(value).replace(/[\t\n\f\r ]/g, "");
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(input) || input.length % 4 === 1) {
    throw __nodeInvalidCharacter();
  }
  return NodeBuffer.from(input, "base64").toString("latin1");
}
function nodeBtoa(value) {
  if (arguments.length === 0 || typeof value === "symbol") {
    throw new TypeError("The data is not a string");
  }
  const input = String(value);
  for (let index = 0; index < input.length; index++) {
    if (input.charCodeAt(index) > 255) throw __nodeInvalidCharacter();
  }
  return NodeBuffer.from(input, "latin1").toString("base64");
}
const __nodeEncodeCodePoint = (output, code) => {
  if (code < 0x80) return output.push(code);
  if (code < 0x800) {
    return output.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
  }
  if (code < 0x10000) {
    return output.push(
      0xe0 | (code >> 12),
      0x80 | ((code >> 6) & 0x3f),
      0x80 | (code & 0x3f)
    );
  }
  return output.push(
    0xf0 | (code >> 18),
    0x80 | ((code >> 12) & 0x3f),
    0x80 | ((code >> 6) & 0x3f),
    0x80 | (code & 0x3f)
  );
};
const __nodeReadCodePoint = (input, index) => {
  const code = input.charCodeAt(index);
  if (code < 0xd800 || code > 0xdfff) return [code, index];
  const next = input.charCodeAt(index + 1);
  if (code <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) {
    return [0x10000 + ((code - 0xd800) << 10) + (next - 0xdc00), index + 1];
  }
  return [0xfffd, index];
};
class NodeTextEncoder {
  encode(value) {
    const output = [];
    const input = String(value);
    let ascii = true;
    for (let index = 0; index < input.length; index++) {
      if (input.charCodeAt(index) > 0x7f) {
        ascii = false;
        break;
      }
    }
    if (ascii) {
      const encoded = new Uint8Array(input.length);
      for (let index = 0; index < input.length; index++) {
        encoded[index] = input.charCodeAt(index);
      }
      return encoded;
    }
    for (let index = 0; index < input.length; index++) {
      const [code, nextIndex] = __nodeReadCodePoint(input, index);
      index = nextIndex;
      __nodeEncodeCodePoint(output, code);
    }
    return new Uint8Array(output);
  }
}
globalThis.TextEncoder = NodeTextEncoder;
const __nodeWindows1252 = {
  128: "€",
  130: "‚",
  131: "ƒ",
  132: "„",
  133: "…",
  134: "†",
  135: "‡",
  136: "ˆ",
  137: "‰",
  138: "Š",
  139: "‹",
  140: "Œ",
  142: "Ž",
  145: "‘",
  146: "’",
  147: "“",
  148: "”",
  149: "•",
  150: "–",
  151: "—",
  152: "˜",
  153: "™",
  154: "š",
  155: "›",
  156: "œ",
  158: "ž",
  159: "Ÿ"
};
class NodeTextDecoder {
  constructor(encoding = "utf-8") {
    this.encoding = String(encoding).toLowerCase();
  }
  decode(bytes) {
    let result = "";
    for (let i = 0; i < bytes.length;) {
      const first = bytes[i++];
      if (this.encoding === "windows-1252" && first >= 128) {
        result += __nodeWindows1252[first] || String.fromCodePoint(first);
      } else if (first < 0x80) result += String.fromCodePoint(first);
      else if (first < 0xe0) {
        result += String.fromCodePoint(
          ((first & 0x1f) << 6) | (bytes[i++] & 0x3f)
        );
      } else if (first < 0xf0) {
        result += String.fromCodePoint(
          ((first & 0x0f) << 12) |
            ((bytes[i++] & 0x3f) << 6) |
            (bytes[i++] & 0x3f)
        );
      } else {
        result += String.fromCodePoint(
          ((first & 7) << 18) |
            ((bytes[i++] & 0x3f) << 12) |
            ((bytes[i++] & 0x3f) << 6) |
            (bytes[i++] & 0x3f)
        );
      }
    }
    return result;
  }
}
globalThis.TextDecoder = NodeTextDecoder;
const nodePathFromURL = (value) => {
  if (value.protocol !== "file:") {
    const error = new TypeError("The URL must use the file: protocol");
    error.code = "ERR_INVALID_URL_SCHEME";
    throw error;
  }
  return globalThis.__nodeUrlModule.fileURLToPath(value);
};
const nodePathValue = (value) =>
  value instanceof NodeBuffer
    ? value.toString()
    : value instanceof Uint8Array
      ? new NodeTextDecoder().decode(value)
      : value instanceof globalThis.__nodeURL
        ? nodePathFromURL(value)
        : String(value);
const nodeFsPath = (value) => {
  if (
    typeof value === "string" ||
    value instanceof NodeBuffer ||
    value instanceof Uint8Array ||
    value instanceof globalThis.__nodeURL
  ) {
    return nodePathValue(value);
  }
  const error = new TypeError(
    'The "path" argument must be of type string or an instance of Buffer or URL.'
  );
  error.message += globalThis.__nodeCommon.invalidArgTypeHelper(value);
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};

globalThis.__nodeAssert = (value, message) => {
  if (!value) __nodeAssertionFailure(message);
};
globalThis.__nodeAssertSkipPrototype = false;
globalThis.__nodeAssert.AssertionError = class AssertionError extends Error {
  constructor(message) {
    super(message);
    this.name = "AssertionError";
  }
};
const __nodeAssertionFailure = (message) => {
  if (message instanceof Error) throw message;
  const error = new globalThis.__nodeAssert.AssertionError(
    message || "Assertion failed"
  );
  error.code = "ERR_ASSERTION";
  error.diff = "simple";
  throw error;
};
const __nodeAssertMissingArgs = () => {
  const error = new TypeError(
    'The "actual" and "expected" arguments must be specified'
  );
  error.code = "ERR_MISSING_ARGS";
  throw error;
};
const __nodeAssertStringDiff = (actual, expected) => {
  const render = (value, marker) => {
    const lines = String(value).split("\n");
    if (lines.at(-1) === "") lines.pop();
    return lines.map((line, index) => {
      return `${marker}   '${line}\\n'${
        index === lines.length - 1 ? "" : " +"
      }`;
    });
  };
  return (
    [
      "Expected values to be strictly equal:",
      "+ actual - expected",
      "",
      ...render(actual, "+"),
      ...render(expected, "-")
    ].join("\n") + "\n"
  );
};
globalThis.__nodeAssert.strictEqual = (actual, expected, message) => {
  if (!Object.is(actual, expected)) {
    const detail =
      actual instanceof Error && expected instanceof Error
        ? `Expected "actual" to be reference-equal to "expected":\n+ actual - expected\n\n+ [${actual.name}: ${actual.message}]\n- [${expected.name}: ${expected.message}]\n`
        : actual &&
            expected &&
            typeof actual === "object" &&
            typeof expected === "object"
          ? `Expected "actual" to be reference-equal to "expected":\n+ actual\n- expected\n`
          : typeof actual === "string" && typeof expected === "string"
            ? actual.includes("\n") || expected.includes("\n")
              ? __nodeAssertStringDiff(actual, expected)
              : `Expected values to be strictly equal:\n+ actual - expected\n\n+ '${actual}'\n- '${expected}'\n`
            : `${actual} !== ${expected}`;
    const error = new globalThis.__nodeAssert.AssertionError(message || detail);
    error.code = "ERR_ASSERTION";
    error.operator = "strictEqual";
    error.actual = actual;
    error.expected = expected;
    error.generatedMessage = message === undefined;
    throw error;
  }
};
globalThis.__nodeAssert.equal = (actual, expected, message) => {
  if (actual != expected) {
    __nodeAssertionFailure(message || `${actual} != ${expected}`);
  }
};
globalThis.__nodeAssert.notStrictEqual = (actual, expected, message) => {
  if (Object.is(actual, expected)) {
    const rendered =
      typeof actual === "string" && actual.length > 20
        ? `\n\n'${actual}'`
        : ` ${actual}`;
    const error = new globalThis.__nodeAssert.AssertionError(
      message || `Expected "actual" to be strictly unequal to:${rendered}`
    );
    error.code = "ERR_ASSERTION";
    error.operator = "notStrictEqual";
    error.actual = actual;
    error.expected = expected;
    error.generatedMessage = message === undefined;
    throw error;
  }
};
globalThis.__nodeAssert.notEqual = (actual, expected, message) => {
  if (actual == expected) {
    const error = new globalThis.__nodeAssert.AssertionError(
      message || `${actual} != ${expected}`
    );
    error.operator = "!=";
    throw error;
  }
};
globalThis.__nodeAssert.ok = globalThis.__nodeAssert;
globalThis.__nodeAssert.fail = (message) => {
  if (message instanceof Error) throw message;
  const generatedMessage = message === undefined;
  const error = new globalThis.__nodeAssert.AssertionError(
    generatedMessage ? "Failed" : String(message)
  );
  error.code = "ERR_ASSERTION";
  error.operator = "fail";
  error.actual = undefined;
  error.expected = undefined;
  error.generatedMessage = generatedMessage;
  throw error;
};
const __nodeAssertNormalizeView = (value, seen) => {
  const values = [];
  let length = 0;
  try {
    length = value.length || 0;
  } catch (_) {}
  for (let index = 0; index < length; index++) {
    values.push(__nodeAssertNormalize(value[index], seen));
  }
  return globalThis.__nodeAssertSkipPrototype
    ? { values }
    : { constructor: value.constructor.name, values };
};
const __nodeAssertNormalizeObject = (value, seen) => {
  const normalized = {};
  if (
    !globalThis.__nodeAssertSkipPrototype &&
    Object.getPrototypeOf(value) !== null &&
    value.constructor !== Object
  ) {
    normalized.__nodeConstructor = value.constructor?.name || "Object";
  }
  for (const key of Object.keys(value).sort()) {
    normalized[key] = __nodeAssertNormalize(value[key], seen);
  }
  return normalized;
};
const __nodeAssertNormalize = (value, seen = new WeakSet(), loose = false) => {
  if (value === undefined) return { __nodeUndefined: true };
  if (typeof value === "bigint") return `${value}n`;
  if (typeof value === "number" && Object.is(value, -0)) {
    return loose ? 0 : { __nodeNegativeZero: true };
  }
  if (typeof value === "number" && Number.isNaN(value)) {
    return { __nodeNaN: true };
  }
  if (value === null || typeof value !== "object") return value;
  if (seen.has(value)) return "[Circular]";
  seen.add(value);
  if (value === globalThis) return { __nodeType: "global" };
  if (globalThis.process && value === globalThis.process) {
    return { __nodeType: "process" };
  }
  if (value instanceof Date) {
    try {
      return {
        constructor: "Date",
        value: new Date(Date.prototype.getTime.call(value)).toISOString()
      };
    } catch (_) {
      // Objects with Date.prototype are intentionally not real Dates.
    }
  }
  if (ArrayBuffer.isView(value)) {
    const values = [];
    for (let index = 0; index < (value.length || 0); index++) {
      values.push(__nodeAssertNormalize(value[index], seen, loose));
    }
    return globalThis.__nodeAssertSkipPrototype
      ? { values }
      : { constructor: value.constructor.name, values };
  }
  if (Array.isArray(value)) {
    return value.map((item) => __nodeAssertNormalize(item, seen, loose));
  }
  if (value instanceof Error) {
    const normalized = {
      constructor: value.constructor.name,
      name: value.name,
      message: value.message
    };
    if ("cause" in value) {
      normalized.cause = __nodeAssertNormalize(value.cause, seen, loose);
    }
    for (const key of Object.keys(value).sort()) {
      normalized[key] = __nodeAssertNormalize(value[key], seen, loose);
    }
    return normalized;
  }
  const normalized = {};
  if (
    !globalThis.__nodeAssertSkipPrototype &&
    Object.getPrototypeOf(value) !== null &&
    value.constructor !== Object
  ) {
    normalized.__nodeConstructor = value.constructor?.name || "Object";
  }
  for (const key of Object.keys(value).sort()) {
    normalized[key] = __nodeAssertNormalize(value[key], seen, loose);
  }
  return normalized;
};
const __nodeAssertErrorLabel = (error) =>
  `[${error.constructor.name}: ${error.message}]`;
const __nodeAssertInspect = (value) => {
  if (value instanceof Error) return __nodeAssertErrorLabel(value);
  return __nodeUtilInspectValue(value, true);
};
const __nodeAssertCauseLines = (cause, prefix) => {
  if (
    cause &&
    typeof cause === "object" &&
    !Array.isArray(cause) &&
    !(cause instanceof Error)
  ) {
    const entries = Object.keys(cause).map(
      (key) => `${prefix}    ${key}: ${__nodeAssertInspect(cause[key])}`
    );
    return [`${prefix}  [cause]: {`, ...entries, `${prefix}  }`];
  }
  return [`${prefix}  [cause]: ${__nodeAssertInspect(cause)}`];
};
const __nodeAssertErrorLines = (error, prefix) => {
  const label = __nodeAssertErrorLabel(error);
  if (!("cause" in error)) return [`${prefix}${label}`];
  return [
    `${prefix}${label} {`,
    ...__nodeAssertCauseLines(error.cause, prefix),
    `${prefix}}`
  ];
};
const __nodeAssertErrorDiff = (actual, expected) => {
  const actualHasCause = "cause" in actual;
  const expectedHasCause = "cause" in expected;
  if (
    actual.message === expected.message &&
    actualHasCause &&
    expectedHasCause
  ) {
    return [
      `  ${__nodeAssertErrorLabel(actual)} {`,
      ...__nodeAssertCauseLines(actual.cause, "+ "),
      ...__nodeAssertCauseLines(expected.cause, "- "),
      "  }"
    ].join("\n");
  }
  const lines = [];
  for (const line of __nodeAssertErrorLines(actual, "+ ")) lines.push(line);
  for (const line of __nodeAssertErrorLines(expected, "- ")) lines.push(line);
  return lines.join("\n");
};
const __nodeAssertDateLabel = (value) => {
  try {
    return value instanceof Date
      ? new Date(Date.prototype.getTime.call(value)).toISOString()
      : "Date {}";
  } catch (_) {
    return "Date {}";
  }
};
const __nodeAssertObjectDiff = (actual, expected) => {
  const keys = [
    ...new Set([...Object.keys(actual), ...Object.keys(expected)])
  ].sort();
  const lines = ["  {"];
  for (const key of keys) {
    if (!Object.prototype.hasOwnProperty.call(actual, key)) continue;
    if (!Object.prototype.hasOwnProperty.call(expected, key)) {
      lines.push(`+   ${key}: ${__nodeAssertInspect(actual[key])}`);
      continue;
    }
    if (
      JSON.stringify(__nodeAssertNormalize(actual[key])) !==
      JSON.stringify(__nodeAssertNormalize(expected[key]))
    ) {
      lines.push(`+   ${key}: ${__nodeAssertInspect(actual[key])}`);
    }
  }
  for (const key of keys) {
    if (!Object.prototype.hasOwnProperty.call(expected, key)) continue;
    if (!Object.prototype.hasOwnProperty.call(actual, key)) {
      lines.push(`-   ${key}: ${__nodeAssertInspect(expected[key])}`);
    } else if (
      JSON.stringify(__nodeAssertNormalize(actual[key])) !==
      JSON.stringify(__nodeAssertNormalize(expected[key]))
    ) {
      lines.push(`-   ${key}: ${__nodeAssertInspect(expected[key])}`);
    }
  }
  lines.push("  }");
  return lines.join("\n");
};
globalThis.__nodeAssert.deepStrictEqual = (actual, expected, message) => {
  if (
    JSON.stringify(__nodeAssertNormalize(actual)) !==
    JSON.stringify(__nodeAssertNormalize(expected))
  ) {
    let detail = "values differ";
    if (actual instanceof Error && expected instanceof Error) {
      detail = `Expected values to be strictly deep-equal:\n+ actual - expected\n\n${__nodeAssertErrorDiff(
        actual,
        expected
      )}`;
    } else if (actual instanceof Date || expected instanceof Date) {
      detail = `Expected values to be strictly deep-equal:\n+ actual - expected\n\n+ ${__nodeAssertDateLabel(
        actual
      )}\n- ${__nodeAssertDateLabel(expected)}\n`;
    } else if (Array.isArray(actual) && Array.isArray(expected)) {
      const lines = [
        "Expected values to be strictly deep-equal:",
        "+ actual - expected",
        "",
        "  ["
      ];
      const length = Math.max(actual.length, expected.length);
      for (let index = 0; index < length; index++) {
        const suffix = index === length - 1 ? "" : ",";
        if (
          index < actual.length &&
          index < expected.length &&
          JSON.stringify(__nodeAssertNormalize(actual[index])) ===
            JSON.stringify(__nodeAssertNormalize(expected[index]))
        ) {
          lines.push(`    ${actual[index]}${suffix}`);
        } else {
          if (index < actual.length) {
            lines.push(`+   ${actual[index]}${suffix}`);
          }
          if (index < expected.length) {
            lines.push(`-   ${expected[index]}${suffix}`);
          }
        }
      }
      lines.push("  ]", "");
      detail = lines.join("\n");
    }
    const customDetail =
      message &&
      actual &&
      expected &&
      typeof actual === "object" &&
      typeof expected === "object"
        ? `${message}\n+ actual - expected\n\n${__nodeAssertObjectDiff(
            actual,
            expected
          )}\n`
        : message;
    const renderedDetail = detail.endsWith("\n") ? detail : `${detail}\n`;
    const error = new globalThis.__nodeAssert.AssertionError(
      customDetail || renderedDetail
    );
    error.code = "ERR_ASSERTION";
    error.operator = "deepStrictEqual";
    error.actual = actual;
    error.expected = expected;
    error.generatedMessage = message === undefined;
    throw error;
  }
};
globalThis.__nodeAssert.deepEqual = (actual, expected, message) => {
  if (
    JSON.stringify(__nodeAssertNormalize(actual, new WeakSet(), true)) ===
    JSON.stringify(__nodeAssertNormalize(expected, new WeakSet(), true))
  ) {
    return;
  }
  const detail =
    typeof actual === "string" && typeof expected === "string"
      ? `Expected values to be loosely deep-equal:\n\n'${
          actual.endsWith("\n") ? actual.slice(0, -1) : actual
        }'\n\nshould loosely deep-equal\n\n'${
          expected.endsWith("\n") ? expected.slice(0, -1) : expected
        }'`
      : message || "values differ";
  const error = new globalThis.__nodeAssert.AssertionError(detail);
  error.code = "ERR_ASSERTION";
  error.operator = "deepEqual";
  error.actual = actual;
  error.expected = expected;
  error.generatedMessage = message === undefined;
  throw error;
};
const __nodeAssertPartialEqual = (actual, expected) => {
  if (ArrayBuffer.isView(actual) && ArrayBuffer.isView(expected)) {
    if (actual.constructor !== expected.constructor) return false;
    if ((actual.length || 0) < (expected.length || 0)) return false;
    if (actual.length === expected.length) {
      return (
        JSON.stringify(__nodeAssertNormalize(actual)) ===
        JSON.stringify(__nodeAssertNormalize(expected))
      );
    }
    for (let index = 0; index < (expected.length || 0); index++) {
      const left = actual[index];
      const right = expected[index];
      if (!(
        Object.is(left, right) ||
        (Number.isNaN(left) && Number.isNaN(right))
      )) {
        return false;
      }
    }
    return true;
  }
  const isShared = (value) =>
    typeof SharedArrayBuffer !== "undefined" &&
    value instanceof SharedArrayBuffer;
  const isBuffer = (value) => value instanceof ArrayBuffer || isShared(value);
  if (isBuffer(actual) && isBuffer(expected)) {
    if (actual.constructor !== expected.constructor) return false;
    if (actual.byteLength < expected.byteLength) return false;
    const left = new Uint8Array(actual);
    const right = new Uint8Array(expected);
    return right.every((byte, index) => left[index] === byte);
  }
  return (
    JSON.stringify(__nodeAssertNormalize(actual)) ===
    JSON.stringify(__nodeAssertNormalize(expected))
  );
};
globalThis.__nodeAssert.partialDeepStrictEqual = (
  actual,
  expected,
  message
) => {
  if (__nodeAssertPartialEqual(actual, expected)) return;
  if (
    ArrayBuffer.isView(actual) ||
    ArrayBuffer.isView(expected) ||
    actual instanceof ArrayBuffer ||
    expected instanceof ArrayBuffer ||
    (typeof SharedArrayBuffer !== "undefined" &&
      (actual instanceof SharedArrayBuffer ||
        expected instanceof SharedArrayBuffer))
  ) {
    __nodeAssertionFailure(message || "values differ");
  }
  return globalThis.__nodeAssert.deepStrictEqual(actual, expected, message);
};
globalThis.__nodeAssert.notDeepStrictEqual = (actual, expected, message) => {
  if (
    JSON.stringify(__nodeAssertNormalize(actual)) ===
    JSON.stringify(__nodeAssertNormalize(expected))
  ) {
    __nodeAssertionFailure(message || "values are deeply equal");
  }
};
globalThis.__nodeAssert.notDeepEqual =
  globalThis.__nodeAssert.notDeepStrictEqual;
const __nodeAssertMatchExpectedFunction = (error, expected) => {
  const isConstructor =
    expected === Array ||
    expected === Error ||
    expected.prototype instanceof Error;
  if (isConstructor && !(error instanceof expected)) {
    const assertion = new globalThis.__nodeAssert.AssertionError(
      `The error is expected to be an instance of "${expected.name}". Received "${error.constructor.name}"\n\nError message:\n\n${error.message}`
    );
    assertion.generatedMessage = true;
    assertion.code = "ERR_ASSERTION";
    assertion.operator = "throws";
    assertion.actual = error;
    assertion.expected = expected;
    throw assertion;
  }
  if (!isConstructor) {
    let matched;
    try {
      matched = expected(error);
    } catch (predicateError) {
      throw predicateError;
    }
    if (!matched) {
      throw error;
    }
  }
};
const __nodeAssertMatchExpectedRegExp = (error, expected) => {
  if (expected instanceof RegExp && !expected.test(String(error))) {
    const assertion = new globalThis.__nodeAssert.AssertionError(
      `The input did not match the regular expression ${expected}. Input:\n\n'${String(
        error
      )}'\n`
    );
    assertion.code = "ERR_ASSERTION";
    assertion.operator = "throws";
    assertion.actual = error;
    assertion.expected = expected;
    throw assertion;
  }
};
const __nodeAssertMatchExpectedObject = (error, expected) => {
  if (!expected || typeof expected !== "object") return;
  for (const key of [
    "name",
    "message",
    "code",
    "operator",
    "actual",
    "expected",
    "generatedMessage"
  ]) {
    if (!(key in expected)) continue;
    if (
      expected[key] instanceof RegExp
        ? !expected[key].test(String(error[key]))
        : error[key] !== expected[key]
    ) {
      throw error;
    }
  }
};
const __nodeAssertMatchExpected = (error, expected) => {
  if (typeof expected === "function") {
    return __nodeAssertMatchExpectedFunction(error, expected);
  }
  __nodeAssertMatchExpectedRegExp(error, expected);
  __nodeAssertMatchExpectedObject(error, expected);
};
globalThis.__nodeAssert.throws = (fn, expected, message) => {
  let thrown = false;
  let captured;
  try {
    fn();
  } catch (error) {
    thrown = true;
    captured = error;
    __nodeAssertMatchExpected(error, expected);
  }
  if (!thrown) {
    const label =
      typeof expected === "function"
        ? ` (${expected.name})`
        : typeof expected === "string"
          ? `: ${expected}`
          : ".";
    const assertion = new globalThis.__nodeAssert.AssertionError(
      `Missing expected exception${label}${message ? `: ${message}` : ""}`
    );
    assertion.code = "ERR_ASSERTION";
    assertion.operator = "throws";
    assertion.actual = undefined;
    assertion.expected = expected;
    if (typeof assertion.stack === "string") {
      assertion.stack = assertion.stack
        .split("\n")
        .filter((line) => !line.includes("throws"))
        .join("\n");
    }
    throw assertion;
  }
  return captured;
};
globalThis.__nodeAssert.ifError = (error) => {
  if (error !== null && error !== undefined) {
    let rendered;
    if (error && error.message !== undefined && error.message !== "") {
      rendered = error.message;
    } else if (error instanceof Error && error.name) rendered = error.name;
    else if (error && error.message !== undefined) rendered = error.message;
    else if (error && typeof error === "object") {
      rendered = `{ ${Object.keys(error)
        .map((key) => `${key}: ${String(error[key])}`)
        .join(", ")} }`;
    } else rendered = String(error);
    const assertion = new globalThis.__nodeAssert.AssertionError(
      `ifError got unwanted exception: ${rendered}`
    );
    assertion.actual = error;
    assertion.expected = null;
    assertion.operator = "ifError";
    assertion.code = "ERR_ASSERTION";
    throw assertion;
  }
};
globalThis.__nodeAssert.doesNotThrow = (fn, expected, message) => {
  try {
    fn();
  } catch (error) {
    if (typeof expected === "function" && !(error instanceof expected)) {
      throw error;
    }
    const text = typeof expected === "string" ? expected : message;
    const assertion = new globalThis.__nodeAssert.AssertionError(
      `Got unwanted exception${
        text ? `: ${text}` : "."
      }\nActual message: "${error.message}"`
    );
    assertion.code = "ERR_ASSERTION";
    assertion.operator = "doesNotThrow";
    throw assertion;
  }
};
globalThis.__nodeAssert.rejects = (promiseOrFn, expected, message) => {
  let input;
  if (typeof promiseOrFn === "function") {
    try {
      input = promiseOrFn();
    } catch (error) {
      return Promise.reject(error);
    }
    if (!input || typeof input.then !== "function") {
      const error = new TypeError(
        `Expected instance of Promise to be returned from the "promiseFn" function but got ${
          input === undefined
            ? "undefined"
            : `an instance of ${input?.constructor?.name || typeof input}`
        }.`
      );
      error.code = "ERR_INVALID_RETURN_VALUE";
      return Promise.reject(error);
    }
    if (typeof input === "function") {
      const error = new TypeError(
        'Expected instance of Promise to be returned from the "promiseFn" function but got a function.'
      );
      error.code = "ERR_INVALID_RETURN_VALUE";
      return Promise.reject(error);
    }
  } else {
    input = promiseOrFn;
    if (!input || typeof input.then !== "function") {
      const error = new TypeError(
        `The "promiseFn" argument must be of type function or an instance of Promise. Received ${
          typeof input === "string"
            ? `type string ('${input}')`
            : `an instance of ${input?.constructor?.name || typeof input}`
        }`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      return Promise.reject(error);
    }
    if (typeof input.catch !== "function") {
      const error = new TypeError(
        'The "promiseFn" argument must be a function or an instance of Promise'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      return Promise.reject(error);
    }
  }
  return Promise.resolve(input).then(
    () => {
      const assertion = new globalThis.__nodeAssert.AssertionError(
        `Missing expected rejection${
          typeof expected === "function"
            ? ` (${expected.name || "mustNotCall"})`
            : ""
        }.`
      );
      assertion.code = "ERR_ASSERTION";
      assertion.operator = "rejects";
      throw assertion;
    },
    (error) => {
      if (typeof expected === "function") {
        let valid;
        try {
          valid = expected(error);
        } catch (validationError) {
          throw validationError;
        }
        if (valid !== true) {
          const assertion = new globalThis.__nodeAssert.AssertionError(
            `The "validate" validation function is expected to return "true". Received '${valid}'\n\nCaught error:\n\n${error}`
          );
          assertion.code = "ERR_ASSERTION";
          assertion.operator = "rejects";
          assertion.generatedMessage = true;
          assertion.actual = error;
          assertion.expected = expected;
          assertion.stack += "\n    at Function.rejects";
          throw assertion;
        }
      }
      if (expected && typeof expected === "object") {
        const mismatch = Object.keys(expected).find((key) => {
          const wanted = expected[key];
          const received = error == null ? undefined : error[key];
          return wanted instanceof RegExp
            ? !wanted.test(String(received))
            : received !== wanted;
        });
        if (mismatch !== undefined) {
          const assertion = new globalThis.__nodeAssert.AssertionError(
            message ||
              expected.message ||
              error?.message ||
              "The input did not match"
          );
          assertion.code = "ERR_ASSERTION";
          assertion.operator = "rejects";
          assertion.generatedMessage = message === undefined;
          assertion.actual = error;
          assertion.expected = expected;
          assertion.stack += "\n    at Function.rejects";
          throw assertion;
        }
      }
      return error;
    }
  );
};
globalThis.__nodeAssert.doesNotReject = (promiseOrFn, message) =>
  Promise.resolve()
    .then(() => {
      let input;
      if (typeof promiseOrFn === "function") {
        try {
          input = promiseOrFn();
        } catch (error) {
          error.__quenchSyncThrow = true;
          throw error;
        }
      } else input = promiseOrFn;
      if (!input || typeof input.then !== "function") {
        const error = new TypeError(
          typeof promiseOrFn === "function"
            ? `Expected instance of Promise to be returned from the "promiseFn" function but got ${
                input === undefined
                  ? "undefined"
                  : `an instance of ${input?.constructor?.name || typeof input}`
              }.`
            : `The "promiseFn" argument must be of type function or an instance of Promise. Received type ${typeof input} (${input})`
        );
        error.code =
          typeof promiseOrFn === "function"
            ? "ERR_INVALID_RETURN_VALUE"
            : "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (typeof input === "function") {
        const error = new TypeError(
          'Expected instance of Promise to be returned from the "promiseFn" function but got a function.'
        );
        error.code = "ERR_INVALID_RETURN_VALUE";
        throw error;
      }
      if (typeof input.catch !== "function") {
        const error = new TypeError(
          'The "promiseFn" argument must be a function or an instance of Promise'
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      return input;
    })
    .catch((error) => {
      if (
        error.code === "ERR_INVALID_RETURN_VALUE" ||
        error.code === "ERR_INVALID_ARG_TYPE" ||
        error.__quenchSyncThrow
      ) {
        throw error;
      }
      const assertion = new globalThis.__nodeAssert.AssertionError(
        message && typeof message !== "function"
          ? message
          : `Got unwanted rejection.\nActual message: "${
              error.message || error
            }"`
      );
      assertion.code = "ERR_ASSERTION";
      assertion.operator = "doesNotReject";
      assertion.actual = error;
      if (typeof message === "function" && !error.__quenchSyncThrow) {
        message(error);
      }
      throw assertion;
    });
globalThis.__nodeAssert.match = (value, expression) => {
  if (!(expression instanceof RegExp)) {
    const received =
      typeof expression === "string"
        ? ` Received type string ('${expression}')`
        : ` Received ${expression}`;
    const error = new TypeError(
      `The "regexp" argument must be an instance of RegExp.${received}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!expression.test(String(value))) {
    throw new Error("Value did not match expression");
  }
};
globalThis.__nodeAssert.doesNotMatch = (value, expression) => {
  if (!(expression instanceof RegExp)) {
    const received =
      typeof expression === "string"
        ? ` Received type string ('${expression}')`
        : ` Received ${expression}`;
    const error = new TypeError(
      `The "regexp" argument must be an instance of RegExp.${received}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (expression.test(String(value))) {
    throw new Error("Value matched expression");
  }
};
globalThis.__nodeAssert.Assert = class Assert {
  constructor(options = {}) {
    this.strict = options?.strict !== false;
    this.skipPrototype = options?.skipPrototype === true;
    if (
      options &&
      options.diff !== undefined &&
      !["simple", "full"].includes(options.diff)
    ) {
      const error = new TypeError(
        `The property 'options.diff' must be one of: 'simple', 'full'. Received '${options.diff}'`
      );
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    this.diff = options?.diff || "simple";
    this.AssertionError = globalThis.__nodeAssert.AssertionError;
    const methods = [
      "ok",
      "fail",
      "equal",
      "notEqual",
      "strictEqual",
      "notStrictEqual",
      "deepEqual",
      "notDeepEqual",
      "deepStrictEqual",
      "notDeepStrictEqual",
      "partialDeepStrictEqual",
      "throws",
      "doesNotThrow",
      "rejects",
      "doesNotReject",
      "ifError",
      "match",
      "doesNotMatch"
    ];
    const owner = this;
    for (const name of methods) {
      this[name] = function (...args) {
        const context =
          this === owner ? owner : { strict: owner.strict, diff: "simple" };
        if (
          [
            "deepEqual",
            "notDeepEqual",
            "deepStrictEqual",
            "notDeepStrictEqual",
            "partialDeepStrictEqual"
          ].includes(name) &&
          args.length < 2
        ) {
          return __nodeAssertMissingArgs();
        }
        const invoke = (callback) => {
          const previousSkipPrototype = globalThis.__nodeAssertSkipPrototype;
          globalThis.__nodeAssertSkipPrototype = owner.skipPrototype;
          try {
            return callback();
          } catch (error) {
            if (error instanceof globalThis.__nodeAssert.AssertionError) {
              error.diff = context.diff;
              if (error.operator === undefined) error.operator = name;
              if (!context.strict) {
                if (name === "deepEqual") error.operator = "deepEqual";
                if (name === "notDeepEqual") error.operator = "notDeepEqual";
              }
              if (
                name === "deepEqual" &&
                context.diff === "simple" &&
                error.generatedMessage &&
                typeof error.actual === "string" &&
                typeof error.expected === "string"
              ) {
                const shorten = (value) => {
                  if (value.includes("\n")) {
                    return value.split("\n").slice(0, 52).join("\n");
                  }
                  return value.length > 511
                    ? `${value.slice(0, 508)}...`
                    : value;
                };
                error.message = `Expected values to be loosely deep-equal:\n\n'${shorten(
                  error.actual
                )}\n\nshould loosely deep-equal\n\n'${shorten(error.expected)}`;
              }
              if (
                name === "notStrictEqual" &&
                context.diff === "simple" &&
                error.generatedMessage &&
                typeof error.actual === "string" &&
                error.actual.includes("\n")
              ) {
                error.message = error.message
                  .split("\n")
                  .slice(0, 50)
                  .join("\n");
              }
            }
            throw error;
          } finally {
            globalThis.__nodeAssertSkipPrototype = previousSkipPrototype;
          }
        };
        if (name === "partialDeepStrictEqual") {
          return invoke(() => globalThis.__nodeAssert.deepStrictEqual(...args));
        }
        if (!context.strict && name === "equal") {
          return invoke(() => globalThis.__nodeAssert.equal(...args));
        }
        if (context.strict && name === "equal") {
          return invoke(() => globalThis.__nodeAssert.strictEqual(...args));
        }
        if (!context.strict && name === "notEqual") {
          return invoke(() => globalThis.__nodeAssert.notEqual(...args));
        }
        if (context.strict && name === "notEqual") {
          return invoke(() => globalThis.__nodeAssert.notStrictEqual(...args));
        }
        return invoke(() => globalThis.__nodeAssert[name](...args));
      };
    }
    if (this.strict) {
      this.equal = this.strictEqual;
      this.notEqual = this.notStrictEqual;
      this.deepEqual = this.deepStrictEqual;
      this.notDeepEqual = this.notDeepStrictEqual;
    }
  }
};
const __nodeAssertClass = globalThis.__nodeAssert.Assert;
globalThis.__nodeAssert.Assert = function Assert(options) {
  if (!new.target) {
    const error = new TypeError(
      "Class constructor Assert cannot be invoked without 'new'"
    );
    error.code = "ERR_CONSTRUCT_CALL_REQUIRED";
    throw error;
  }
  return Reflect.construct(__nodeAssertClass, [options], new.target);
};
globalThis.__nodeAssert.Assert.prototype = __nodeAssertClass.prototype;
globalThis.__nodeAssert.strict = globalThis.__nodeAssert;

const __nodePathArg = (value) => {
  if (typeof value !== "string") {
    const error = new TypeError('The "path" argument must be of type string');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  return value;
};
globalThis.atob ||= nodeAtob;
globalThis.btoa ||= nodeBtoa;
