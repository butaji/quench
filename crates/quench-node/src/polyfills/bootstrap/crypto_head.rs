//! Polyfill: `crypto-head`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeCryptoValidatePbkdf2Types = (password, salt) => {
  const isBinary = (value) =>
    ArrayBuffer.isView(value) ||
    value instanceof ArrayBuffer ||
    (typeof SharedArrayBuffer !== "undefined" &&
      value instanceof SharedArrayBuffer);
  if (typeof password !== "string" && !isBinary(password)) {
    throw Object.assign(new TypeError('The "password" argument must be of type string or an instance of Buffer'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof salt !== "string" && !isBinary(salt)) {
    throw Object.assign(new TypeError('The "salt" argument must be of type string or an instance of Buffer'), { code: "ERR_INVALID_ARG_TYPE" });
  }
};
const __nodeCryptoKeylenReceived = (keylen) => {
  if (typeof keylen === "string") return ` Received type string ('${keylen}')`;
  if (keylen === null || keylen === undefined) return ` Received ${keylen}`;
  return ` Received an instance of ${keylen.constructor?.name || "Object"}`;
};
const __nodeCryptoNumberReceived = (value) => {
  if (typeof value === "string") return ` Received type string ('${value}')`;
  if (value === null || value === undefined) return ` Received ${value}`;
  if (typeof value === "boolean") return ` Received type boolean (${value})`;
  return ` Received an instance of ${value.constructor?.name || "Object"}`;
};
const __nodeCryptoValidatePbkdf2Numbers = (iterations, keylen) => {
  if (typeof iterations !== "number") {
    const error = new TypeError(
      `The "iterations" argument must be of type number.${
        __nodeCryptoNumberReceived(
          iterations,
        )
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    !Number.isInteger(iterations) ||
    iterations <= 0 ||
    iterations > 0x7fffffff
  ) {
    throw Object.assign(new RangeError('The value of "iterations" is out of range'), { code: "ERR_OUT_OF_RANGE" });
  }
  if (typeof keylen !== "number") {
    const error = new TypeError(
      `The "keylen" argument must be of type number.${
        __nodeCryptoKeylenReceived(
          keylen,
        )
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(keylen) || keylen < 0 || keylen > 0x7fffffff) {
    throw Object.assign(new RangeError(`The value of "keylen" is out of range. It must be an integer. Received ${keylen}`), { code: "ERR_OUT_OF_RANGE" });
  }
};
"#);
