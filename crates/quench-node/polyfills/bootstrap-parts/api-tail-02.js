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
        throw Object.assign(new TypeError('Expected instance of Promise to be returned from the "promiseFn" function but got a function.'), { code: "ERR_INVALID_RETURN_VALUE" });
      }
      if (typeof input.catch !== "function") {
        throw Object.assign(new TypeError('The "promiseFn" argument must be a function or an instance of Promise'), { code: "ERR_INVALID_ARG_TYPE" });
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
    throw Object.assign(new TypeError(`The "regexp" argument must be an instance of RegExp.${received}`), { code: "ERR_INVALID_ARG_TYPE" });
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
    throw Object.assign(new TypeError(`The "regexp" argument must be an instance of RegExp.${received}`), { code: "ERR_INVALID_ARG_TYPE" });
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
      throw Object.assign(new TypeError(`The property 'options.diff' must be one of: 'simple', 'full'. Received '${options.diff}'`), { code: "ERR_INVALID_ARG_VALUE" });
    }
    this.diff = options?.diff || "simple";
    this.AssertionError = globalThis.__nodeAssert.AssertionError;
    const methods =
      "ok fail equal notEqual strictEqual notStrictEqual deepEqual notDeepEqual deepStrictEqual notDeepStrictEqual partialDeepStrictEqual throws doesNotThrow rejects doesNotReject ifError match doesNotMatch".split(
        " "
      );
    const owner = this;
    for (const name of methods) {
      this[name] = function (...args) {
        const context =
          this === owner ? owner : { strict: owner.strict, diff: "simple" };
        if (
          "deepEqual notDeepEqual deepStrictEqual notDeepStrictEqual partialDeepStrictEqual"
            .split(" ")
            .includes(name) &&
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
    throw Object.assign(new TypeError("Class constructor Assert cannot be invoked without 'new'"), { code: "ERR_CONSTRUCT_CALL_REQUIRED" });
  }
  return Reflect.construct(__nodeAssertClass, [options], new.target);
};
globalThis.__nodeAssert.Assert.prototype = __nodeAssertClass.prototype;
globalThis.__nodeAssert.strict = globalThis.__nodeAssert;
const __nodePathArg = (value) => {
  if (typeof value !== "string") {
    throw Object.assign(new TypeError('The "path" argument must be of type string'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  return value;
};
globalThis.atob ||= nodeAtob;
globalThis.btoa ||= nodeBtoa;
