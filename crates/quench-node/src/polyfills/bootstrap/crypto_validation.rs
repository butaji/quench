//! Polyfill: `crypto-validation`

pub const JS: &str = r#"const __nodeCryptoRandomArguments = (minimum, maximum, callback) => {
  if (typeof minimum === "function") {
    return { minimum: 0, maximum: 0x1_0000_0000_0000, callback: minimum };
  }
  if (typeof maximum === "function") {
    return { minimum: 0, maximum: minimum, callback: maximum };
  }
  if (maximum === undefined) return { minimum: 0, maximum: minimum, callback };
  return { minimum, maximum, callback };
};
const __nodeCryptoCipherInfo = (name) =>
  String(name).toLowerCase() === "aes-128-cbc"
    ? {
        name: "aes-128-cbc",
        nid: 419,
        blockSize: 16,
        ivLength: 16,
        keyLength: 16,
        mode: "cbc"
      }
    : undefined;
const __nodeCryptoAssertDigestOpen = (finalized) => {
  if (finalized) {
    const error = new Error("Digest already called");
    error.code = "ERR_CRYPTO_HASH_FINALIZED";
    throw error;
  }
};
const __nodeCryptoValidateStringEncoding = (value, encoding) => {
  if (encoding === "hex" && value.length % 2) {
    throw Object.assign(new TypeError(`The argument 'encoding' is invalid for data of length ${value.length}. Received 'hex'`), { code: "ERR_INVALID_ARG_VALUE" });
  }
};
const __nodeCryptoSetPrototype = (value, constructor) =>
  typeof constructor === "function" &&
  Object.setPrototypeOf(value, constructor.prototype);
const __quenchValidateDhGenerator = (generator) => {
  if (
    (typeof generator === "number" && generator <= 1) ||
    (generator &&
      typeof generator.byteLength === "number" &&
      (generator.byteLength === 0 || generator[0] <= 1))
  ) {
    const error = new Error("bad generator");
    error.code = "ERR_OSSL_DH_BAD_GENERATOR";
    throw error;
  }
};
const __quenchValidateDhInputType = (value) => {
  if (
    typeof value !== "number" &&
    typeof value !== "string" &&
    !(value instanceof ArrayBuffer) &&
    !ArrayBuffer.isView(value)
  ) {
    throw Object.assign(
      new TypeError(
        "The sizeOrKey argument must be a number, string, or buffer"
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (typeof value === "string" && value.length === 0) {
    throw Object.assign(new TypeError("The sizeOrKey argument is invalid"), {
      code: "ERR_INVALID_ARG_TYPE"
    });
  }
};
const __quenchValidateDhGeneratorType = (generator) => {
  if (
    generator !== undefined &&
    typeof generator !== "number" &&
    typeof generator !== "string" &&
    !(generator instanceof ArrayBuffer) &&
    !ArrayBuffer.isView(generator)
  ) {
    throw Object.assign(new TypeError("The generator argument is invalid"), {
      code: "ERR_INVALID_ARG_TYPE"
    });
  }
};
const __quenchValidateDhNumbers = (sizeOrKey, generator) => {
  __quenchValidateDhInputType(sizeOrKey);
  __quenchValidateDhGeneratorType(generator);
  if (typeof sizeOrKey === "number" && !Number.isInteger(sizeOrKey)) {
    throw Object.assign(
      new RangeError(
        `The value of "sizeOrKey" is out of range. It must be an integer. Received ${sizeOrKey}`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
  if (typeof sizeOrKey === "number" && sizeOrKey <= 1) {
    const error = new Error("modulus too small");
    error.code = "ERR_OSSL_DH_MODULUS_TOO_SMALL";
    throw error;
  }
  if (typeof generator === "number" && !Number.isInteger(generator)) {
    throw Object.assign(
      new RangeError(
        `The value of "generator" is out of range. It must be an integer. Received ${generator}`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
  __quenchValidateDhGenerator(generator);
};
const __nodeCryptoHmacDigest = (algorithm, inner, outer, chunks, encoding) => {
  const message = [];
  for (const chunk of chunks) message.push(...chunk);
  const innerDigest = globalThis.__quench_digest_bytes(algorithm, [
    ...inner,
    ...message
  ]);
  const result = NodeBuffer.from(
    globalThis.__quench_digest_bytes(algorithm, [...outer, ...innerDigest])
  );
  if (encoding === undefined || encoding === null) return result;
  if (
    "hex base64 latin1 binary utf8 utf-8 ucs2 ucs-2 utf16le utf-16le"
      .split(" ")
      .includes(encoding)
  ) {
    return result.toString(encoding);
  }
  throw Object.assign(new TypeError(`Unknown encoding: ${encoding}`), { code: "ERR_UNKNOWN_ENCODING" });
};
"#;
