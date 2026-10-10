pub(crate) const ASSERT_REJECTS: &str = r#"(() => { const nativeResolve = Promise.resolve; const nativeThen = Promise.prototype.then; return (promiseOrFn, expected, message) => {
  const receivedType = (value) => {
    if (value === undefined) return "undefined";
    if (value === null) return "null";
    if (typeof value === "string") return `type string ('${value}')`;
    if (typeof value === "number") return `type number (${value})`;
    if (typeof value === "boolean") return `type boolean (${value})`;
    if (typeof value === "object" && value.constructor && value.constructor.name)
      return `an instance of ${value.constructor.name}`;
    return `type ${typeof value}`;
  };
  let input;
  if (typeof promiseOrFn === "function") {
    try { input = promiseOrFn(); }
    catch (error) { return Promise.reject(error); }
    if (!(input instanceof Promise)) {
      const error = new TypeError(`Expected instance of Promise to be returned from the \"promiseFn\" function but got ${receivedType(input)}.`);
      error.code = "ERR_INVALID_RETURN_VALUE";
      return Promise.reject(error);
    }
  } else input = promiseOrFn;
  if (!input || typeof input.then !== "function") {
    const error = new TypeError(`The \"promiseFn\" argument must be of type function or an instance of Promise. Received ${receivedType(promiseOrFn)}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    return Promise.reject(error);
  }
  if (typeof input.catch !== "function") {
    const error = new TypeError(`The \"promiseFn\" argument must be of type function or an instance of Promise. Received ${receivedType(promiseOrFn)}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    return Promise.reject(error);
  }
  return nativeThen.call(nativeResolve.call(Promise, input),
    () => {
      return Promise.reject(Object.assign(new (require("assert").AssertionError)({message: message || `Missing expected rejection${typeof expected === "function" ? ` (${expected.name || "mustNotCall"})` : ""}.`}), {
      code: "ERR_ASSERTION", operator: "rejects", generatedMessage: !message
      }));
    },
    (error) => {
      if (typeof expected === "function") {
        const validation = expected(error);
        if (validation !== true) {
          const received = typeof validation === "string" ? `'${validation}'` : String(validation);
          const caught = error && typeof error.name === "string"
            ? `${error.name}: ${error.message || ""}`
            : String(error);
          const validationMessage = `The "validate" validation function is expected to return "true". Received ${received}\n\nCaught error:\n\n${caught}`;
          return Promise.reject(Object.assign(new (require("assert").AssertionError)({message: validationMessage}), {
          code: "ERR_ASSERTION", operator: "rejects", actual: error, expected, generatedMessage: true, stack: "AssertionError: The rejection did not match\\n    at Function.rejects"
          }));
        }
      }
      if (expected && typeof expected === "object") {
        const rejectsMatch = (received, wanted) => {
          if (
            received &&
            wanted &&
            typeof received === "object" &&
            typeof wanted === "object" &&
            (received instanceof Error || wanted instanceof Error ||
              received instanceof DOMException || wanted instanceof DOMException)
          ) {
            if (String(received.name) !== String(wanted.name)) return false;
            if (String(received.message) !== String(wanted.message)) return false;
            if ("code" in wanted && received.code !== wanted.code) return false;
            return true;
          }
          return received === wanted;
        };
        for (const key of Object.keys(expected)) {
          const expectedValue = expected[key];
          const actualValue = error == null ? undefined : error[key];
          const matches = expectedValue instanceof RegExp
            ? expectedValue.test(actualValue)
            : rejectsMatch(actualValue, expectedValue);
          if (!matches) {
            return Promise.reject(Object.assign(new (require("assert").AssertionError)({message: message || "The input did not match"}), {
              code: "ERR_ASSERTION", operator: "rejects", generatedMessage: !message, actual: error, expected, stack: `AssertionError: ${message || "The input did not match"}\\n    at Function.rejects`
            }));
          }
        }
      }
      return error;
    }
  );
}; })()"#;
