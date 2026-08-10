const __nodeCryptoValidatePbkdf2Digest = (digest) => {
  if (typeof digest !== "string") {
    const received = digest === undefined
      ? "undefined"
      : digest === null
      ? "null"
      : typeof digest;
    const error = new TypeError(
      `The "digest" argument must be of type string. Received ${received}`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (digest.toLowerCase() !== "sha256") {
    const error = new TypeError(`Invalid digest: ${digest}`);
    error.code = "ERR_CRYPTO_INVALID_DIGEST";
    throw error;
  }
};
const __nodeCryptoValidatePbkdf2 = (
  password,
  salt,
  iterations,
  keylen,
  digest,
) => {
  __nodeCryptoValidatePbkdf2Types(password, salt);
  __nodeCryptoValidatePbkdf2Numbers(iterations, keylen);
  __nodeCryptoValidatePbkdf2Digest(digest);
};
const __nodeCryptoPbkdf2Bytes = (value) => {
  if (typeof value === "string") return new NodeTextEncoder().encode(value);
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (
    typeof SharedArrayBuffer !== "undefined" &&
    value instanceof SharedArrayBuffer
  ) {
    return new Uint8Array(value);
  }
  return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
};
const __nodeCryptoHashCopy = (algorithm, chunks) => {
  const clone = globalThis.__nodeCrypto.createHash(algorithm);
  for (const chunk of chunks) clone.update(chunk);
  return clone;
};
const __nodeCryptoHmacPads = (algorithm, key) => {
  const blockSize = ["sha384", "sha512"].includes(algorithm) ? 128 : 64;
  let keyBytes = typeof key === "string"
    ? new NodeTextEncoder().encode(key)
    : NodeBuffer.from(key);
  if (keyBytes.length > blockSize) {
    keyBytes = NodeBuffer.from(
      globalThis.__quench_digest_bytes(algorithm, Array.from(keyBytes)),
    );
  }
  const padded = NodeBuffer.alloc(blockSize);
  padded.set(keyBytes);
  const inner = new NodeBuffer(blockSize);
  const outer = new NodeBuffer(blockSize);
  for (let index = 0; index < blockSize; index++) {
    inner[index] = padded[index] ^ 0x36;
    outer[index] = padded[index] ^ 0x5c;
  }
  return { keyBytes, inner, outer };
};
globalThis.__nodeCryptoInitialized = false;
const __nodeCryptoApi = {
  getHashes: () => [
    "RSA-SHA1",
    "md5",
    "sha1",
    "sha224",
    "sha256",
    "sha384",
    "sha512",
  ],
  getCiphers: () => ["aes-128-cbc"],
  getCipherInfo: __nodeCryptoCipherInfo,
  getCurves: () => ["secp384r1"],
  timingSafeEqual: (left, right) => {
    if (!(left instanceof Uint8Array) || !(right instanceof Uint8Array)) {
      const error = new TypeError(
        'The "buf1" and "buf2" arguments must be instances of Buffer or Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (left.length !== right.length) {
      const error = new RangeError(
        "Input buffers must have the same byte length",
      );
      error.code = "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH";
      throw error;
    }
    let difference = 0;
    for (let index = 0; index < left.length; index++) {
      difference |= left[index] ^ right[index];
    }
    return difference === 0;
  },
  randomInt: (minimum = 0, maximum, callback) => {
    const oneArgument = maximum === undefined || typeof maximum === "function";
    ({ minimum, maximum, callback } = __nodeCryptoRandomArguments(
      minimum,
      maximum,
      callback,
    ));
    if (callback !== undefined && typeof callback !== "function") {
      throw globalThis.__nodeCryptoRandomCallbackError();
    }
    if (!Number.isSafeInteger(minimum) || !Number.isSafeInteger(maximum)) {
      const error = globalThis.__nodeCryptoRandomIntegerError(minimum, maximum);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const rangeError = globalThis.__nodeCryptoRandomIntegerRangeError(
      minimum,
      maximum,
      oneArgument,
    );
    if (rangeError) throw rangeError;
    const range = maximum - minimum;
    const limit = Math.floor(0x1_0000_0000_0000 / range) * range;
    const choose = () => {
      let value;
      do {
        const bytes = globalThis.__quench_random_bytes(6);
        value = 0;
        for (const byte of bytes) value = value * 256 + byte;
      } while (value >= limit);
      return minimum + (value % range);
    };
    if (typeof callback === "function") {
      queueMicrotask(() => callback(null, choose()));
      return;
    }
    return choose();
  },
  randomUUID: () => globalThis.__quench_random_uuid(),
  randomBytes: (size, callback) => {
    if (!Number.isInteger(size) || size < 0 || size > 0x7fffffff) {
      const error = new RangeError('The "size" argument is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (callback !== undefined && typeof callback !== "function") {
      const error = new TypeError(
        'The "callback" argument must be of type function',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const output = NodeBuffer.from(globalThis.__quench_random_bytes(size));
    if (typeof callback === "function") {
      queueMicrotask(() => callback(null, output));
    }
    return output;
  },
  pseudoRandomBytes: (...args) => __nodeCryptoRandomBytes(...args),
  randomFillSync: (buffer, offset = 0, size = buffer.length - offset) => {
    if (!ArrayBuffer.isView(buffer)) {
      const error = new TypeError(
        'The "buffer" argument must be an instance of ArrayBufferView',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      !Number.isSafeInteger(offset) ||
      !Number.isSafeInteger(size) ||
      offset < 0 ||
      size < 0 ||
      offset + size > buffer.byteLength
    ) {
      const error = new RangeError('The value of "offset" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const bytes = globalThis.__quench_random_bytes(Number(size));
    buffer.set(bytes, offset);
    return buffer;
  },
  randomFill: (buffer, offset, size, callback) => {
    if (typeof offset === "function") {
      callback = offset;
      offset = 0;
      size = buffer.length;
    } else if (typeof size === "function") {
      callback = size;
      size = buffer.length - (offset || 0);
    }
    if (typeof callback !== "function") {
      throw new TypeError('The "callback" argument must be of type function');
    }
    try {
      const result = globalThis.__nodeCrypto.randomFillSync(
        buffer,
        offset || 0,
        size === undefined ? buffer.length - (offset || 0) : size,
      );
      queueMicrotask(() => callback(null, result));
    } catch (error) {
      queueMicrotask(() => callback(error));
    }
  },
  pbkdf2Sync: (password, salt, iterations, keylen, digest) => {
    __nodeCryptoValidatePbkdf2(password, salt, iterations, keylen, digest);
    return NodeBuffer.from(
      __quench_pbkdf2_bytes(
        Array.from(__nodeCryptoPbkdf2Bytes(password)),
        Array.from(__nodeCryptoPbkdf2Bytes(salt)),
        iterations,
        keylen,
      ),
    );
  },
  pbkdf2: (password, salt, iterations, keylen, digest, callback) => {
    if (typeof digest === "function") {
      callback = digest;
      digest = undefined;
    }
    if (typeof callback !== "function") {
      const error = new TypeError(
        'The "callback" argument must be of type function',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    __nodeCryptoValidatePbkdf2(password, salt, iterations, keylen, digest);
    let result;
    try {
      result = globalThis.__nodeCrypto.pbkdf2Sync(
        password,
        salt,
        iterations,
        keylen,
        digest,
      );
    } catch (error) {
      queueMicrotask(() => callback(error));
      return;
    }
    queueMicrotask(() => callback(null, result));
  },
  // eslint-disable-next-line max-lines-per-function, complexity -- hash stream methods share one state object
  createHash: (algorithm, options) => {
    if (typeof algorithm !== "string") {
      throw Object.assign(
        new TypeError(
          `The "algorithm" argument must be of type string. Received ${algorithm}`,
        ),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    // prettier-ignore
    const normalized = algorithm.toLowerCase(),
      isXof = normalized.startsWith("shake");
    const digestName = normalized === "rsa-sha1" ? "sha1" : normalized;
    // prettier-ignore
    if (
      !isXof && options?.outputLength !== undefined &&
      !(normalized === "sha224" && options.outputLength === 28)
    ) {
      if (typeof options.outputLength !== "number") {
        throw Object.assign(
          new TypeError("The outputLength option must be a number"),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      throw Object.assign(
        options.outputLength === 28
          ? new Error("not XOF or invalid length")
          : new RangeError("outputLength is out of range"),
        {
          code: options.outputLength === 28
            ? "ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH"
            : "ERR_OUT_OF_RANGE",
        },
      );
    }
    // prettier-ignore
    if (isXof && (!options || typeof options.outputLength !== "number")) {
      throw Object.assign(new Error("not XOF or invalid length"), {
        code: "ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH",
      });
    }
    if (
      !isXof &&
      !["sha1", "sha224", "sha256", "sha384", "sha512", "md5"].includes(
        digestName,
      )
    ) {
      throw new Error("Digest method not supported");
    }
    const chunks = [];
    const listeners = { data: [], end: [] };
    let finalized = false;
    let streamOutput;
    const hash = {
      _writableState: { defaultEncoding: options?.defaultEncoding || "utf8" },
      on: (event, listener) => {
        if (listeners[event]) listeners[event].push(listener);
        return hash;
      },
      update: (value, encoding) => {
        __nodeCryptoAssertDigestOpen(finalized);
        if (value === undefined) {
          // prettier-ignore
          throw Object.assign(
            new TypeError(
              "The data argument must be of type string or an instance of Buffer",
            ),
            { code: "ERR_INVALID_ARG_TYPE" },
          );
        }
        if (typeof value === "string") {
          __nodeCryptoValidateStringEncoding(value, encoding);
          chunks.push(NodeBuffer.from(value, encoding || "utf8"));
        } else if (value instanceof Uint8Array) chunks.push(value);
        else {
          throw Object.assign(
            new TypeError(
              "The data argument must be of type string or an instance of Buffer",
            ),
            { code: "ERR_INVALID_ARG_TYPE" },
          );
        }
        return hash;
      },
      write: (value, encoding) => (
        hash.update(value, encoding || hash._writableState.defaultEncoding),
          true
      ),
      end: (value, encoding) => {
        if (value !== undefined) hash.update(value, encoding);
        streamOutput = hash.digest();
        for (const listener of listeners.data) listener(streamOutput);
        for (const listener of listeners.end) listener();
        return hash;
      },
      read: () => {
        if (streamOutput !== undefined) return streamOutput;
        return hash.digest();
      },
      digest: (encoding) => {
        __nodeCryptoAssertDigestOpen(finalized);
        finalized = true;
        const input = [];
        for (const chunk of chunks) input.push(...chunk);
        const bytes = NodeBuffer.from(
          isXof
            ? globalThis.__quench_shake_bytes(
              digestName,
              input,
              options.outputLength,
            )
            : globalThis.__quench_digest_bytes(digestName, input),
        );
        if (
          encoding === undefined ||
          encoding === null ||
          encoding === "buffer"
        ) {
          return bytes;
        }
        if (encoding === "hex") return bytes.toString("hex");
        if (encoding === "base64") return bytes.toString("base64");
        if (encoding === "latin1") return bytes.toString("latin1");
        if (
          ["utf8", "utf-8", "ucs2", "ucs-2", "utf16le", "utf-16le"].includes(
            encoding,
          )
        ) {
          return bytes.toString(encoding);
        }
        throw Object.assign(new TypeError(`Unknown encoding: ${encoding}`), {
          code: "ERR_UNKNOWN_ENCODING",
        });
      },
      copy: (copyOptions) => {
        __nodeCryptoAssertDigestOpen(finalized);
        if (isXof) {
          if (!copyOptions || typeof copyOptions.outputLength !== "number") {
            const error = new Error("not XOF or invalid length");
            error.code = "ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH";
            throw error;
          }
          const clone = __nodeCryptoApi.createHash(normalized, copyOptions);
          for (const chunk of chunks) clone.update(chunk);
          return clone;
        }
        return __nodeCryptoHashCopy(normalized, chunks);
      },
    };
    __nodeCryptoSetPrototype(hash, globalThis.__quenchHashConstructor);
    return hash;
  },
  Hash: function Hash(algorithm) {
    return __nodeCryptoApi.createHash(algorithm);
  },
  // eslint-disable-next-line max-lines-per-function -- HMAC state methods share one object
  createHmac: (algorithm, key) => {
    if (typeof algorithm !== "string") {
      throw Object.assign(
        new TypeError(
          `The "hmac" argument must be of type string. Received ${algorithm}`,
        ),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    if (key === null || key === undefined) {
      throw Object.assign(
        new TypeError(
          'The "key" argument must be of type string or an instance of Buffer',
        ),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    algorithm = algorithm.toLowerCase() === "dss1"
      ? "sha1"
      : algorithm.toLowerCase();
    if (
      !["md5", "sha1", "sha224", "sha256", "sha384", "sha512"].includes(
        algorithm,
      )
    ) {
      throw Object.assign(new TypeError(`Invalid digest: ${algorithm}`), {
        code: "ERR_CRYPTO_INVALID_DIGEST",
      });
    }
    const { keyBytes, inner, outer } = __nodeCryptoHmacPads(algorithm, key);
    const chunks = [];
    let finalized = false;
    let streamOutput;
    const hmac = {
      update: (value, encoding) => {
        if (finalized) {
          const error = new Error("Digest already called");
          error.code = "ERR_CRYPTO_HASH_FINALIZED";
          throw error;
        }
        if (typeof value === "string") {
          __nodeCryptoValidateStringEncoding(value, encoding);
          chunks.push(NodeBuffer.from(value, encoding || "utf8"));
        } else chunks.push(NodeBuffer.from(value));
        return hmac;
      },
      end: (value, encoding) => {
        if (value !== undefined) hmac.update(value, encoding);
        streamOutput = hmac.digest();
        return hmac;
      },
      read: () => {
        if (streamOutput !== undefined) return streamOutput;
        return hmac.digest();
      },
      digest: (encoding) => {
        __nodeCryptoAssertDigestOpen(finalized);
        finalized = true;
        return __nodeCryptoHmacDigest(
          algorithm,
          inner,
          outer,
          chunks,
          encoding,
        );
      },
      copy: () => {
        if (finalized) {
          const error = new Error("Digest already called");
          error.code = "ERR_CRYPTO_HASH_FINALIZED";
          throw error;
        }
        const clone = globalThis.__nodeCrypto.createHmac(algorithm, keyBytes);
        for (const chunk of chunks) clone.update(chunk);
        return clone;
      },
    };
    __nodeCryptoSetPrototype(hmac, globalThis.__quenchHmacConstructor);
    return hmac;
  },
};
