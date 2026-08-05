globalThis.Buffer = new Proxy(NodeBuffer, {
  apply(_target, _thisArg, args) {
    if (typeof args[0] === "number")
      return new NodeBuffer(NodeBuffer._validateSize(args[0]));
    return NodeBuffer.from(...args);
  },
  construct(_target, args) {
    if (typeof args[0] === "number") {
      if (args.length > 1) {
        const error = new TypeError(
          `The "string" argument must be of type string. Received type number (${args[0]})`
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      return new NodeBuffer(NodeBuffer._validateSize(args[0]));
    }
    return NodeBuffer.from(...args);
  }
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
  )
    for (const name of __nodeGetOwnPropertyNames(prototype))
      if (
        !name.startsWith("_") &&
        typeof Object.getOwnPropertyDescriptor(prototype, name)?.value ===
          "function"
      )
        names.add(name);
  return Array.from(names);
};
const nodeAtob = (value) => NodeBuffer.from(String(value), "base64").toString();
const nodeBtoa = (value) => NodeBuffer.from(String(value)).toString("base64");
const __nodeEncodeCodePoint = (output, code) => {
  if (code < 0x80) return output.push(code);
  if (code < 0x800)
    return output.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
  if (code < 0x10000)
    return output.push(
      0xe0 | (code >> 12),
      0x80 | ((code >> 6) & 0x3f),
      0x80 | (code & 0x3f)
    );
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
  if (code <= 0xdbff && next >= 0xdc00 && next <= 0xdfff)
    return [0x10000 + ((code - 0xd800) << 10) + (next - 0xdc00), index + 1];
  return [0xfffd, index];
};
class NodeTextEncoder {
  encode(value) {
    const output = [];
    const input = String(value);
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
      if (this.encoding === "windows-1252" && first >= 128)
        result += __nodeWindows1252[first] || String.fromCodePoint(first);
      else if (first < 0x80) result += String.fromCodePoint(first);
      else if (first < 0xe0)
        result += String.fromCodePoint(
          ((first & 0x1f) << 6) | (bytes[i++] & 0x3f)
        );
      else if (first < 0xf0)
        result += String.fromCodePoint(
          ((first & 0x0f) << 12) |
            ((bytes[i++] & 0x3f) << 6) |
            (bytes[i++] & 0x3f)
        );
      else
        result += String.fromCodePoint(
          ((first & 7) << 18) |
            ((bytes[i++] & 0x3f) << 12) |
            ((bytes[i++] & 0x3f) << 6) |
            (bytes[i++] & 0x3f)
        );
    }
    return result;
  }
}
globalThis.TextDecoder = NodeTextDecoder;
const nodePathValue = (value) =>
  value instanceof NodeBuffer
    ? value.toString()
    : value instanceof Uint8Array
      ? new NodeTextDecoder().decode(value)
      : value instanceof globalThis.__nodeURL
        ? globalThis.__nodeUrlModule.fileURLToPath(value)
        : String(value);
const nodeFsPath = (value) => {
  if (
    typeof value === "string" ||
    value instanceof NodeBuffer ||
    value instanceof Uint8Array ||
    value instanceof globalThis.__nodeURL
  )
    return nodePathValue(value);
  const error = new TypeError(
    'The "path" argument must be of type string or an instance of Buffer or URL'
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};

globalThis.__nodeAssert = (value, message) => {
  if (!value) __nodeAssertionFailure(message);
};
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
  throw error;
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
          : `${actual} !== ${expected}`;
    __nodeAssertionFailure(message || detail);
  }
};
globalThis.__nodeAssert.equal = (actual, expected, message) => {
  if (actual != expected)
    __nodeAssertionFailure(message || `${actual} != ${expected}`);
};
globalThis.__nodeAssert.notStrictEqual = (actual, expected, message) => {
  if (Object.is(actual, expected)) {
    const rendered =
      typeof actual === "string" && actual.length > 20
        ? `\n\n'${actual}'`
        : ` ${actual}`;
    __nodeAssertionFailure(
      message || `Expected "actual" to be strictly unequal to:${rendered}`
    );
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
globalThis.__nodeAssert.fail = (message = "Failed") =>
  __nodeAssertionFailure(message);
const __nodeAssertNormalizeView = (value, seen) => {
  const values = [];
  let length = 0;
  try {
    length = value.length || 0;
  } catch (_) {}
  for (let index = 0; index < length; index++)
    values.push(__nodeAssertNormalize(value[index], seen));
  return { constructor: value.constructor.name, values };
};
const __nodeAssertNormalizeObject = (value, seen) => {
  const normalized = {};
  for (const key of Object.keys(value).sort())
    normalized[key] = __nodeAssertNormalize(value[key], seen);
  return normalized;
};
const __nodeAssertNormalize = (value, seen = new WeakSet()) => {
  if (typeof value === "bigint") return `${value}n`;
  if (value === null || typeof value !== "object") return value;
  if (seen.has(value)) return "[Circular]";
  seen.add(value);
  if (ArrayBuffer.isView(value)) return __nodeAssertNormalizeView(value, seen);
  if (Array.isArray(value))
    return value.map((item) => __nodeAssertNormalize(item, seen));
  return __nodeAssertNormalizeObject(value, seen);
};
globalThis.__nodeAssert.deepStrictEqual = (actual, expected, message) => {
  if (
    JSON.stringify(__nodeAssertNormalize(actual)) !==
    JSON.stringify(__nodeAssertNormalize(expected))
  ) {
    __nodeAssertionFailure(message || "values differ");
  }
};
globalThis.__nodeAssert.deepEqual = globalThis.__nodeAssert.deepStrictEqual;
globalThis.__nodeAssert.notDeepStrictEqual = (actual, expected, message) => {
  if (
    JSON.stringify(__nodeAssertNormalize(actual)) ===
    JSON.stringify(__nodeAssertNormalize(expected))
  )
    __nodeAssertionFailure(message || "values are deeply equal");
};
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
  if (!isConstructor && !expected(error)) throw error;
};
const __nodeAssertMatchExpectedRegExp = (error, expected) => {
  if (expected instanceof RegExp && !expected.test(String(error))) {
    const assertion = new globalThis.__nodeAssert.AssertionError(
      `The input did not match the regular expression ${expected}. Input:\n\n'${String(error)}'\n`
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
  for (const key of ["name", "message", "code", "operator"])
    if (
      expected[key] &&
      (expected[key] instanceof RegExp
        ? !expected[key].test(String(error[key]))
        : error[key] !== expected[key])
    )
      throw error;
};
const __nodeAssertMatchExpected = (error, expected) => {
  if (typeof expected === "function")
    return __nodeAssertMatchExpectedFunction(error, expected);
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
    throw assertion;
  }
  return captured;
};
globalThis.__nodeAssert.ifError = (error) => {
  if (error) throw error;
};
globalThis.__nodeAssert.doesNotThrow = (fn, expected, message) => {
  try {
    fn();
  } catch (error) {
    const text = typeof expected === "string" ? expected : message;
    const assertion = new globalThis.__nodeAssert.AssertionError(
      `Got unwanted exception${text ? `: ${text}` : "."}\nActual message: "${error.message}"`
    );
    assertion.code = "ERR_ASSERTION";
    assertion.operator = "doesNotThrow";
    throw assertion;
  }
};
globalThis.__nodeAssert.rejects = (promiseOrFn, expected) =>
  Promise.resolve()
    .then(() =>
      typeof promiseOrFn === "function" ? promiseOrFn() : promiseOrFn
    )
    .then(
      () => {
        throw new Error("Missing expected rejection");
      },
      (error) => {
        if (expected && expected.name && error.name !== expected.name)
          throw error;
        return error;
      }
    );
globalThis.__nodeAssert.doesNotReject = (promiseOrFn, message) =>
  Promise.resolve()
    .then(() =>
      typeof promiseOrFn === "function" ? promiseOrFn() : promiseOrFn
    )
    .catch((error) => {
      throw new Error(message || `Unexpected rejection: ${error}`);
    });
globalThis.__nodeAssert.match = (value, expression) => {
  if (!expression.test(String(value)))
    throw new Error("Value did not match expression");
};
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
