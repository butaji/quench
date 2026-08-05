const __quenchCryptoConstructors = (result) => {
  for (const name of [
    "createPublicKey",
    "createPrivateKey",
    "createDiffieHellman",
    "createECDH",
    "KeyObject",
    "Certificate",
    "X509Certificate",
    "sign",
    "verify",
    "createVerify",
    "generateKeyPair",
    "generateKeyPairSync",
    "generateKey",
    "generateKeySync",
    "createDecipheriv",
    "hkdf",
    "pbkdf2",
    "scrypt",
    "scryptSync"
  ])
    result[name] ||= function Constructor() {};
};
const __quenchCryptoSignFallback = (result) => {
  result.createSign ||= () => ({
    update() {
      return this;
    },
    sign() {
      throw Object.assign(
        new Error("error:02000070:rsa routines::digest too big for rsa key"),
        { library: "rsa routines" }
      );
    }
  });
};
const __quenchCryptoCipherFallback = (result) => {
  result.createCipheriv ||= () => {
    let inputEncoding;
    const validateEncoding = (encoding) => {
      if (encoding && !["utf8", "utf-8", "hex", "base64"].includes(encoding)) {
        const error = new TypeError(`Unknown encoding: ${encoding}`);
        error.code = "ERR_UNKNOWN_ENCODING";
        throw error;
      }
      if (inputEncoding && encoding && encoding !== inputEncoding) {
        const error = new TypeError(
          `Encoding cannot be changed from '${inputEncoding}'`
        );
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      inputEncoding ||= encoding;
    };
    return {
      update(value, encoding, outputEncoding) {
        validateEncoding(encoding);
        validateEncoding(outputEncoding);
        return new Uint8Array(value);
      },
      final(outputEncoding) {
        validateEncoding(outputEncoding);
        return new Uint8Array(0);
      }
    };
  };
};
const __quenchCryptoRandomFallbacks = (result, state) => {
  result.randomBytes ||= (size) => new Uint8Array(Number(size) || 0);
  result.randomFill ||= (buffer, callback) => callback?.(null, buffer);
  result.randomFillSync ||= (buffer) => buffer;
  result.randomInt ||= (min, max) =>
    Math.floor(Math.random() * (Number(max) - Number(min))) + Number(min);
  result.randomUUID ||= () =>
    "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (char) => {
      const value = (Math.random() * 16) | 0;
      return (char === "x" ? value : (value & 3) | 8).toString(16);
    });
  result.getFips ||= () => state.fips;
  result.setFips ||= (value) => {
    state.fips = Number(value) ? 1 : 0;
  };
};
const __quenchCryptoDigestFallbacks = (result) => {
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
      for (let index = 0; index < derived.length; index += 1)
        derived[index] ^= previous[index];
    }
    output.set(derived.subarray(0, output.length - offset), offset);
    offset += derived.length;
  }
  return output;
};
const __quenchCryptoKdfFallbacks = (result) => {
  result.hkdfSync ||= (hash, ikm, salt, info, length) => {
    const output = new Uint8Array(Number(length));
    let previous = new Uint8Array(0);
    for (let index = 1, offset = 0; offset < output.length; index += 1) {
      previous = new Uint8Array(
        result
          .createHmac(hash, salt)
          .update(previous)
          .update(ikm)
          .update(info)
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
  __quenchCryptoRandomFallbacks(result, state);
  __quenchCryptoDigestFallbacks(result);
  __quenchCryptoKdfFallbacks(result);
  __quenchCryptoConstructors(result);
  __quenchCryptoSignFallback(result);
  __quenchCryptoCipherFallback(result);
  __quenchCryptoWebFallbacks(result);
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) => {
    const result = originalRequire(name);
    if (String(name).replace(/^node:/, "") === "crypto")
      return __quenchCryptoFallbacks(result);
    return result;
  };
}
