//! Polyfill: `api-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"globalThis.__nodeAssert.notDeepEqual =
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
  for (const key of "name message code operator actual expected generatedMessage".split(
    " "
  )) {
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
"#);
