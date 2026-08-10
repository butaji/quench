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
  throw Object.assign(new TypeError('The "actual" and "expected" arguments must be specified'), { code: "ERR_MISSING_ARGS" });
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
