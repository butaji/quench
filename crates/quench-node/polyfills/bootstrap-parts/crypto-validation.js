const __nodeCryptoRandomArguments = (minimum, maximum, callback) => {
  if (typeof minimum === "function")
    return { minimum: 0, maximum: 0x1_0000_0000_0000, callback: minimum };
  if (typeof maximum === "function")
    return { minimum: 0, maximum: minimum, callback: maximum };
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
    const error = new TypeError(
      `The argument 'encoding' is invalid for data of length ${value.length}. Received 'hex'`
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
};
const __nodeCryptoSetPrototype = (value, constructor) =>
  typeof constructor === "function" &&
  Object.setPrototypeOf(value, constructor.prototype);
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
  if (encoding === "hex" || encoding === "base64")
    return result.toString(encoding);
  const error = new TypeError(`Unknown encoding: ${encoding}`);
  error.code = "ERR_UNKNOWN_ENCODING";
  throw error;
};
