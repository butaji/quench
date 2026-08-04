const __nodeCryptoRandomArguments = (minimum, maximum, callback) => {
  if (typeof minimum === "function")
    return { minimum: 0, maximum: 0x1_0000_0000_0000, callback: minimum };
  if (typeof maximum === "function")
    return { minimum: 0, maximum: minimum, callback: maximum };
  if (maximum === undefined) return { minimum: 0, maximum: minimum, callback };
  return { minimum, maximum, callback };
};
const __nodeCryptoValidatePbkdf2Types = (password, salt) => {
  if (typeof password !== "string" && !(password instanceof Uint8Array))
    throw new TypeError(
      'The "password" argument must be of type string or an instance of Buffer'
    );
  if (typeof salt !== "string" && !(salt instanceof Uint8Array))
    throw new TypeError(
      'The "salt" argument must be of type string or an instance of Buffer'
    );
};
const __nodeCryptoValidatePbkdf2Numbers = (iterations, keylen) => {
  if (
    !Number.isInteger(iterations) ||
    iterations <= 0 ||
    iterations > 0x7fffffff
  )
    throw new RangeError('The value of "iterations" is out of range');
  if (!Number.isInteger(keylen) || keylen < 0 || keylen > 0x7fffffff)
    throw new RangeError('The value of "keylen" is out of range');
};
const __nodeCryptoValidatePbkdf2Digest = (digest) => {
  if (typeof digest !== "string")
    throw new TypeError('The "digest" argument must be of type string');
  if (digest.toLowerCase() !== "sha256")
    throw new Error(`Unsupported digest: ${digest}`);
};
const __nodeCryptoValidatePbkdf2 = (
  password,
  salt,
  iterations,
  keylen,
  digest
) => {
  __nodeCryptoValidatePbkdf2Types(password, salt);
  __nodeCryptoValidatePbkdf2Numbers(iterations, keylen);
  __nodeCryptoValidatePbkdf2Digest(digest);
};
const __nodeCryptoHashCopy = (chunks) => {
  const clone = globalThis.__nodeCrypto.createHash("sha256");
  for (const chunk of chunks) clone.update(chunk);
  return clone;
};
const __nodeCryptoAssertDigestOpen = (finalized) => {
  if (finalized) {
    const error = new Error("Digest already called");
    error.code = "ERR_CRYPTO_HASH_FINALIZED";
    throw error;
  }
};
const __nodeCryptoHmacDigest = (inner, outer, chunks, encoding) => {
  const message = [];
  for (const chunk of chunks) message.push(...chunk);
  const innerDigest = globalThis.__quench_sha256_bytes([...inner, ...message]);
  const result = NodeBuffer.from(
    globalThis.__quench_sha256_bytes([...outer, ...innerDigest])
  );
  if (encoding === undefined || encoding === null) return result;
  if (encoding === "hex" || encoding === "base64")
    return result.toString(encoding);
  const error = new TypeError(`Unknown encoding: ${encoding}`);
  error.code = "ERR_UNKNOWN_ENCODING";
  throw error;
};
const __nodeCryptoHmacPads = (key) => {
  let keyBytes =
    typeof key === "string"
      ? new NodeTextEncoder().encode(key)
      : NodeBuffer.from(key);
  if (keyBytes.length > 64)
    keyBytes = NodeBuffer.from(
      globalThis.__quench_sha256_bytes(Array.from(keyBytes))
    );
  const padded = NodeBuffer.alloc(64);
  padded.set(keyBytes);
  const inner = new NodeBuffer(64);
  const outer = new NodeBuffer(64);
  for (let index = 0; index < 64; index++) {
    inner[index] = padded[index] ^ 0x36;
    outer[index] = padded[index] ^ 0x5c;
  }
  return { keyBytes, inner, outer };
};
const __createNodeCrypto = () => ({
  getHashes: () => ["sha256"],
  getCiphers: () => [],
  timingSafeEqual: (left, right) => {
    if (!(left instanceof Uint8Array) || !(right instanceof Uint8Array)) {
      const error = new TypeError(
        'The "buf1" and "buf2" arguments must be instances of Buffer or Uint8Array'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (left.length !== right.length) {
      const error = new RangeError(
        "Input buffers must have the same byte length"
      );
      error.code = "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH";
      throw error;
    }
    let difference = 0;
    for (let index = 0; index < left.length; index++)
      difference |= left[index] ^ right[index];
    return difference === 0;
  },
  randomInt: (minimum = 0, maximum, callback) => {
    ({ minimum, maximum, callback } = __nodeCryptoRandomArguments(
      minimum,
      maximum,
      callback
    ));
    if (!Number.isSafeInteger(minimum) || !Number.isSafeInteger(maximum)) {
      const error = new TypeError("The bounds must be safe integers");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (maximum <= minimum || maximum - minimum > 0x1_0000_0000_0000) {
      const error = new RangeError(
        "The difference between max and min must be less than 2^48"
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
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
        'The "callback" argument must be of type function'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const output = NodeBuffer.from(globalThis.__quench_random_bytes(size));
    if (typeof callback === "function")
      queueMicrotask(() => callback(null, output));
    return output;
  },
  randomFillSync: (buffer, offset = 0, size = buffer.length - offset) => {
    if (!ArrayBuffer.isView(buffer)) {
      const error = new TypeError(
        'The "buffer" argument must be an instance of ArrayBufferView'
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
    if (typeof callback !== "function")
      throw new TypeError('The "callback" argument must be of type function');
    try {
      const result = globalThis.__nodeCrypto.randomFillSync(
        buffer,
        offset || 0,
        size === undefined ? buffer.length - (offset || 0) : size
      );
      queueMicrotask(() => callback(null, result));
    } catch (error) {
      queueMicrotask(() => callback(error));
    }
  },
  pbkdf2Sync: (password, salt, iterations, keylen, digest) => {
    __nodeCryptoValidatePbkdf2(password, salt, iterations, keylen, digest);
    const passwordBytes =
      typeof password === "string"
        ? new NodeTextEncoder().encode(password)
        : password;
    const saltBytes =
      typeof salt === "string" ? new NodeTextEncoder().encode(salt) : salt;
    const output = [];
    const blocks = Math.ceil(keylen / 32);
    for (let block = 1; block <= blocks; block++) {
      const suffix = [
        (block >>> 24) & 255,
        (block >>> 16) & 255,
        (block >>> 8) & 255,
        block & 255
      ];
      let u = NodeBuffer.from(
        globalThis.__nodeCrypto
          .createHmac("sha256", passwordBytes)
          .update(NodeBuffer.from([...saltBytes, ...suffix]))
          .digest()
      );
      const result = Array.from(u);
      for (let count = 1; count < iterations; count++) {
        u = NodeBuffer.from(
          globalThis.__nodeCrypto
            .createHmac("sha256", passwordBytes)
            .update(u)
            .digest()
        );
        for (let index = 0; index < result.length; index++)
          result[index] ^= u[index];
      }
      output.push(...result);
    }
    return NodeBuffer.from(output.slice(0, keylen));
  },
  pbkdf2: (password, salt, iterations, keylen, digest, callback) => {
    if (typeof callback !== "function")
      throw new TypeError('The "callback" argument must be of type function');
    __nodeCryptoValidatePbkdf2(password, salt, iterations, keylen, digest);
    let result;
    try {
      result = globalThis.__nodeCrypto.pbkdf2Sync(
        password,
        salt,
        iterations,
        keylen,
        digest
      );
    } catch (error) {
      queueMicrotask(() => callback(error));
      return;
    }
    queueMicrotask(() => callback(null, result));
  },
  createHash: (algorithm) => {
    if (algorithm !== "sha256")
      throw new Error(`Unsupported hash: ${algorithm}`);
    const chunks = [];
    let finalized = false;
    const hash = {
      update: (value, encoding) => {
        __nodeCryptoAssertDigestOpen(finalized);
        if (typeof value === "string")
          chunks.push(NodeBuffer.from(value, encoding || "utf8"));
        else if (value instanceof Uint8Array) chunks.push(value);
        else chunks.push(new NodeTextEncoder().encode(String(value)));
        return hash;
      },
      digest: (encoding) => {
        __nodeCryptoAssertDigestOpen(finalized);
        finalized = true;
        const input = [];
        for (const chunk of chunks) input.push(...chunk);
        const bytes = NodeBuffer.from(globalThis.__quench_sha256_bytes(input));
        const result = bytes.toString("hex");
        if (encoding === undefined || encoding === null) return bytes;
        if (encoding === "hex") return result;
        if (encoding === "base64") return bytes.toString("base64");
        const error = new TypeError(`Unknown encoding: ${encoding}`);
        error.code = "ERR_UNKNOWN_ENCODING";
        throw error;
      },
      copy: () => {
        __nodeCryptoAssertDigestOpen(finalized);
        return __nodeCryptoHashCopy(chunks);
      }
    };
    return hash;
  },
  createHmac: (algorithm, key) => {
    if (algorithm !== "sha256")
      throw new Error(`Unsupported hmac: ${algorithm}`);
    const { keyBytes, inner, outer } = __nodeCryptoHmacPads(key);
    const chunks = [];
    let finalized = false;
    const hmac = {
      update: (value, encoding) => {
        if (finalized) {
          const error = new Error("Digest already called");
          error.code = "ERR_CRYPTO_HASH_FINALIZED";
          throw error;
        }
        chunks.push(
          typeof value === "string"
            ? NodeBuffer.from(value, encoding || "utf8")
            : NodeBuffer.from(value)
        );
        return hmac;
      },
      digest: (encoding) => {
        __nodeCryptoAssertDigestOpen(finalized);
        finalized = true;
        return __nodeCryptoHmacDigest(inner, outer, chunks, encoding);
      },
      copy: () => {
        if (finalized) {
          const error = new Error("Digest already called");
          error.code = "ERR_CRYPTO_HASH_FINALIZED";
          throw error;
        }
        const clone = globalThis.__nodeCrypto.createHmac("sha256", keyBytes);
        for (const chunk of chunks) clone.update(chunk);
        return clone;
      }
    };
    return hmac;
  }
});
let __nodeCryptoInstance;
globalThis.__nodeCryptoInitialized = false;
globalThis.__nodeCrypto = new Proxy(
  {},
  {
    get: (_, key) => {
      globalThis.__nodeCryptoInitialized = true;
      __nodeCryptoInstance ||= __createNodeCrypto();
      return __nodeCryptoInstance[key];
    },
    ownKeys: () =>
      Reflect.ownKeys((__nodeCryptoInstance ||= __createNodeCrypto())),
    getOwnPropertyDescriptor: (_, key) => ({
      enumerable: true,
      configurable: true,
      value: (__nodeCryptoInstance ||= __createNodeCrypto())[key]
    })
  }
);
