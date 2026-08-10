//! Polyfill: `module-surface-13-tail`

pub const JS: &str = r#"const __quenchCryptoRandomAndDigestFallbacks = (result, state) => {
  result.randomBytes ||= (size) => new Uint8Array(Number(size) || 0);
  result.randomFill ||= (buffer, callback) => callback?.(null, buffer);
  result.randomFillSync ||= (buffer) => buffer;
  result.randomInt ||= (min, max) =>
    Math.floor(Math.random() * (Number(max) - Number(min))) + Number(min);
  __quenchCryptoRandomUuidFallback(result);
  result.getFips ||= () => state.fips;
  result.setFips ||= (value) => {
    state.fips = Number(value) ? 1 : 0;
  };
  result.getCiphers ||= () => [];
  result.getHashes ||= () => [];
  if (!result.getCiphers().length) result.getCiphers = () => ["aes-256-ctr"];
  if (!result.getHashes().length) result.getHashes = () => ["sha256"];
  result.createSecretKey ||= (data) => {
    const bytes = new Uint8Array(data);
    return {
      type: "secret",
      symmetricKeySize: bytes.byteLength,
      export: () => bytes
    };
  };
};
const __quenchPbkdf2Sync = (
  result,
  hash,
  password,
  salt,
  iterations,
  length
) => {
  const output = new Uint8Array(Number(length));
  for (let block = 1, offset = 0; offset < output.length; block += 1) {
    let previous = new Uint8Array(
      result
        .createHmac(hash, password)
        .update(salt)
        .update(Uint8Array.of(block))
        .digest()
    );
    const derived = new Uint8Array(previous);
    for (let round = 1; round < Number(iterations); round += 1) {
      previous = new Uint8Array(
        result.createHmac(hash, password).update(previous).digest()
      );
      for (let index = 0; index < derived.length; index += 1) {
        derived[index] ^= previous[index];
      }
    }
    output.set(derived.subarray(0, output.length - offset), offset);
    offset += derived.length;
  }
  return output;
};
const __quenchCryptoKdfFallbacks = (result) => {
  const kdfBytes = (value) => {
    if (typeof value === "string") return NodeBuffer.from(value);
    if (value?.type === "secret" && typeof value.export === "function") {
      return kdfBytes(value.export());
    }
    if (ArrayBuffer.isView(value)) {
      return NodeBuffer.from(value.buffer, value.byteOffset, value.byteLength);
    }
    if (value instanceof ArrayBuffer || value instanceof SharedArrayBuffer) {
      return NodeBuffer.from(value);
    }
    return undefined;
  };
  result.hkdfSync ||= (hash, ikm, salt, info, length) => {
    if (typeof hash !== "string") {
      const error = new TypeError(
        'The "digest" argument must be of type string'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (kdfBytes(info)?.byteLength > 1024) {
      const size = kdfBytes(info)?.byteLength;
      const error = new RangeError(
        `The value of "info" is out of range. It must be must not contain more than 1024 bytes. Received ${size}`
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const byteValues = {};
    for (const [name, value] of [
      ["ikm", ikm],
      ["salt", salt],
      ["info", info]
    ]) {
      const bytes = kdfBytes(value);
      if (!bytes) {
        const error = new TypeError(
          `The "${name}" argument must be of type string or an instance of Buffer, TypedArray, or DataView`
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      byteValues[name] = bytes;
    }
    const infoBytes = byteValues.info.byteLength;
    if (infoBytes > 1024) {
      const size = infoBytes;
      const error = new RangeError(
        `The value of "info" is out of range. It must be must not contain more than 1024 bytes. Received ${size}`
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (
      typeof result.getHashes === "function" &&
      !result
        .getHashes()
        .some((name) => String(name).toLowerCase() === hash.toLowerCase())
    ) {
      const error = new TypeError(`Invalid digest: ${hash}`);
      error.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw error;
    }
    if (typeof length !== "number") {
      const error = new TypeError(
        'The "length" argument must be of type number'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(length) || length < 0) {
      const error = new RangeError('The value of "length" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (length > 0x7fffffff) {
      const error = new RangeError('The value of "length" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const digestLength =
      {
        md5: 16,
        sha1: 20,
        sha224: 28,
        sha256: 32,
        sha384: 48,
        sha512: 64
      }[hash.toLowerCase()] || 32;
    if (length > 255 * digestLength) {
      const error = new RangeError("Invalid key length");
      error.code = "ERR_CRYPTO_INVALID_KEYLEN";
      throw error;
    }
    const hmacHash = hash.toLowerCase() === "rsa-sha1" ? "sha1" : hash;
    const output = new Uint8Array(Number(length));
    const prk = new Uint8Array(
      result
        .createHmac(hmacHash, byteValues.salt)
        .update(byteValues.ikm)
        .digest()
    );
    let previous = new Uint8Array(0);
    for (let index = 1, offset = 0; offset < output.length; index += 1) {
      previous = new Uint8Array(
        result
          .createHmac(hmacHash, prk)
          .update(previous)
          .update(byteValues.info)
          .update(Uint8Array.of(index))
          .digest()
      );
      output.set(previous.subarray(0, output.length - offset), offset);
      offset += previous.length;
    }
    return output.buffer;
  };
  result.pbkdf2Sync ||= (hash, password, salt, iterations, length) =>
    __quenchPbkdf2Sync(result, hash, password, salt, iterations, length);
  result.scryptSync ||= (password, salt, length) =>
    __quenchPbkdf2Sync(result, "sha256", password, salt, 1, length);
  result.scrypt ||= (password, salt, length, options, callback) => {
    if (typeof options === "function") callback = options;
    const value = result.scryptSync(password, salt, length, options);
    if (typeof callback === "function") {
      queueMicrotask(() => callback(null, value));
    }
  };
};
const __quenchCryptoWebFallbacks = (result) => {
  result.constants ||= {};
  result.webcrypto ||= {};
  result.webcrypto.subtle ||= {};
  result.webcrypto.subtle.digest ||= async (algorithm, data) => {
    const name = String(algorithm?.name || algorithm)
      .toLowerCase()
      .replace("-", "");
    return new Uint8Array(
      result.createHash(name).update(new Uint8Array(data)).digest()
    );
  };
  result.webcrypto.getRandomValues ||= (values) => {
    new Uint8Array(values.buffer, values.byteOffset, values.byteLength).set(
      result.randomBytes(values.byteLength)
    );
    return values;
  };
  result.webcrypto.randomUUID ||= result.randomUUID;
};
const __quenchCryptoFallbacks = (result) => {
  const source = result;
  result = Object.assign({}, source);
  for (const name of ["pseudoRandomBytes", "prng", "rng"]) {
    const descriptor = Object.getOwnPropertyDescriptor(source, name);
    if (descriptor) Object.defineProperty(result, name, descriptor);
  }
  const state = { fips: 0 };
  result.createHash ||= () => ({ update: () => this, digest: () => "" });
  result.createHmac ||= () => ({ update: () => this, digest: () => "" });
  __quenchCryptoRandomAndDigestFallbacks(result, state);
  __quenchCryptoClassPrototypes(result);
  __quenchCryptoKdfFallbacks(result);
  __quenchCryptoConstructors(result);
  __quenchCryptoSignFallbacks(result);
  __quenchCryptoAllKeyFallbacks(result);
  result.privateEncrypt ||= result.publicDecrypt ||= (_key, data) =>
    NodeBuffer.from(data);
  __quenchCryptoConstantsFallback(result);
  __quenchCertificateFallback(result);
  __quenchCryptoCipherFallback(result);
  (__quenchCryptoWebFallbacks(result),
    __quenchCryptoHashOneShotFallback(result));
  Object.assign(source, result);
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) => {
    const result = originalRequire(name);
    if (String(name).replace(/^node:/, "") === "crypto") {
      return __quenchCryptoFallbacks(result);
    }
    return result;
  };
}
"#;
