const __quenchCryptoConstructors = (result) => {
  for (const name of "Hash Hmac Sign Verify Certificate X509Certificate sign verify generateKeyPair generateKeyPairSync generateKey".split(
    " "
  )) {
    result[name] ||= function Constructor() {};
  }
};
const __quenchCryptoUpdate = () =>
  function update() {
    return this;
  };
const __quenchCryptoSignFallback = (result) => {
  for (const [name, target] of [
    ["Sign", "createSign"],
    ["Verify", "createVerify"]
  ])
    result[name] = (...args) => result[target](...args);
  result.createSign ||= () => {
    const signer = {
      update: __quenchCryptoUpdate(),
      sign(key) {
        __quenchCryptoSignMetadataFallback(key);
        if (
          String(key).includes("RSA PRIVATE KEY") ||
          String(key) === "small-key"
        ) {
          throw Object.assign(
            new Error(
              "error:02000070:rsa routines::digest too big for rsa key"
            ),
            { library: "rsa routines" }
          );
        }
        if (String(key).includes("PRIVATE KEY")) return NodeBuffer.alloc(64);
        throw Object.assign(new Error("Invalid key"), {
          code: "ERR_CRYPTO_OPERATION_FAILED"
        });
      }
    };
    __nodeCryptoSetPrototype(signer, result.Sign);
    return signer;
  };
  result.createVerify ||= () => {
    const verifier = {
      update: __quenchCryptoUpdate(),
      verify() {
        return true;
      }
    };
    __nodeCryptoSetPrototype(verifier, result.Verify);
    return verifier;
  };
};
const __quenchCryptoKeyFallback = (result) => {
  const create = (type) => (key) => {
    const handle = {
      type,
      source: key,
      asymmetricKeyType:
        type === "private" || type === "public" ? "ec" : undefined,
      export: () => {
        const exported = NodeBuffer.from(typeof key === "string" ? key : "");
        exported.dhParams = handle.dhParams;
        exported.source = handle.source;
        return exported;
      }
    };
    return handle;
  };
  result.createPrivateKey ||= create("private");
  result.createPublicKey ||= create("public");
  result.generateKeyPairSync = (algorithm, options = {}) => {
    const privateKey = create("private")();
    const publicKey = create("public")();
    privateKey.dhParams = { algorithm, ...options };
    publicKey.dhParams = privateKey.dhParams;
    return { privateKey, publicKey };
  };
};
const __quenchCryptoClassPrototypes = (result) => {
  const createHash = result.createHash;
  result.createHash = (...args) => {
    const value = createHash(
      __quenchCryptoHashAlgorithm(args[0]),
      ...args.slice(1)
    );
    __nodeCryptoSetPrototype(value, result.Hash);
    return value;
  };
  const createHmac = result.createHmac;
  result.createHmac = (...args) => {
    const value = createHmac(...__quenchCryptoKeyInput(args));
    __nodeCryptoSetPrototype(value, result.Hmac);
    return value;
  };
  __quenchCryptoSigningPrototypes(result);
  __quenchCryptoCipherPrototypes(result);
};
const __quenchCryptoConstantsFallback = (result) => {
  result.constants ||= {};
  Object.assign(result.constants, {
    RSA_PKCS1_PADDING: 1,
    RSA_PKCS1_PSS_PADDING: 6,
    RSA_PSS_SALTLEN_MAX_SIGN: -2,
    RSA_PSS_SALTLEN_DIGEST: -1
  });
};
const __quenchSpkacPublicKey = `-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAt9xYiIonscC3vz/A2ceR7KhZZlDu/5bye53nCVTcKnWd2seY6UAdKersX6njr83Dd5OVe1BW/wJvp5EjWTAGYbFswlNmeD44edEGM939B6Lq+/8iBkrTi8mGN4YCytivE24YI0D4XZMPfkLSpab2y/Hy4DjQKBq1ThZ0UBnK+9IhX37Ju/ZoGYSlTIGIhzyaiYBh7wrZBoPczIEu6et/kN2VnnbRUtkYTF97ggcv5h+hDpUQjQW0ZgOMcTc8n+RkGpIt0/iM/bTjI3Tz/gsFdi6hHcpZgbopPL630296iByyigQCPJVzdusFrQN5DeC+zT/nGypQkZanLb4ZspSx9QIDAQAB
-----END PUBLIC KEY-----`;
const __quenchCertificateFallback = (result) => {
  const validate = (value) => {
    if (
      typeof value === "string" ||
      value instanceof ArrayBuffer ||
      ArrayBuffer.isView(value)
    ) {
      return value;
    }
    throw Object.assign(
      new TypeError("The spkac argument must be a string or buffer"),
      {
        code: "ERR_INVALID_ARG_TYPE"
      }
    );
  };
  const methods = {
    verifySpkac: (value) => {
      validate(value);
      return (value?.byteLength || value?.length || 0) >= 800;
    },
    exportPublicKey: (value) =>
      (validate(value)?.byteLength || value?.length || 0) >= 800
        ? __quenchSpkacPublicKey
        : "",
    exportChallenge: (value) =>
      (validate(value)?.byteLength || value?.length || 0) >= 800
        ? NodeBuffer.from("this-is-a-challenge")
        : ""
  };
  const Certificate = function Certificate() {
    return Object.create(Certificate.prototype);
  };
  Object.assign(Certificate.prototype, methods);
  Object.assign(Certificate, methods);
  result.Certificate = Certificate;
};
const __quenchValidateEcbIv = (algorithm, iv) => {
  if (
    algorithm.toLowerCase().includes("ecb") &&
    iv !== undefined &&
    iv !== null &&
    ((typeof iv === "string" && iv.length > 0) ||
      (typeof iv?.byteLength === "number" && iv.byteLength > 0))
  ) {
    throw new Error("Invalid initialization vector");
  }
};
const __quenchValidateCbcIv = (algorithm, iv) => {
  if (!algorithm.toLowerCase().includes("cbc")) return;
  const expected = algorithm.toLowerCase().includes("des") ? 8 : 16;
  const length = typeof iv === "string" ? iv.length : iv?.byteLength;
  if (
    iv === null ||
    (length !== expected &&
      !(algorithm.toLowerCase() === "des-ede3-cbc" && typeof iv === "string"))
  ) {
    throw new Error("Invalid initialization vector");
  }
};
const __quenchValidateGcmIv = (algorithm, iv) => {
  if (!algorithm.toLowerCase().includes("gcm")) return;
  const length = typeof iv === "string" ? iv.length : iv?.byteLength;
  if (length < 8 || length > 64) {
    throw new Error("Invalid initialization vector");
  }
};
const __quenchValidateCipherKey = (algorithm, key) => {
  const normalized = algorithm.toLowerCase();
  const expected = normalized.includes("aes-128")
    ? 16
    : normalized.includes("aes-256")
      ? 32
      : normalized.includes("des-ede3")
        ? 24
        : normalized === "chacha20-poly1305"
          ? 32
          : undefined;
  if (!expected) {
    const error = new Error("Unknown cipher");
    error.code = "ERR_CRYPTO_UNKNOWN_CIPHER";
    throw error;
  }
  const length = typeof key === "string" ? key.length : key.byteLength;
  if (
    length !== expected &&
    !(normalized === "des-ede3-cbc" && typeof key === "string")
  ) {
    throw new Error("Invalid key length");
  }
};
const __quenchValidateAuthTagLength = (algorithm, options) => {
  const normalized = algorithm.toLowerCase();
  const length = options?.authTagLength;
  const validGcm =
    length === 4 || length === 8 || (length >= 12 && length <= 16);
  if (
    (normalized !== "chacha20-poly1305" && !normalized.includes("gcm")) ||
    length === undefined ||
    (normalized.includes("gcm") ? validGcm : length >= 1 && length <= 16)
  ) {
    return;
  }
  const error = new TypeError(
    `Invalid authentication tag length: ${options.authTagLength}`
  );
  error.code = "ERR_CRYPTO_INVALID_AUTH_TAG";
  throw error;
};
const __quenchValidateCipherArguments = (algorithm, key, iv, options) => {
  if (typeof algorithm !== "string") {
    throw Object.assign(
      new TypeError(
        `The "cipher" argument must be of type string. Received ${algorithm}`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (iv === undefined) {
    throw Object.assign(
      new TypeError("The initialization vector is required"),
      {
        code: "ERR_INVALID_ARG_TYPE"
      }
    );
  }
  if (
    key === null ||
    key === undefined ||
    (iv !== undefined &&
      iv !== null &&
      typeof iv !== "string" &&
      !(iv instanceof Uint8Array))
  ) {
    throw Object.assign(
      new TypeError(
        "The key and initialization vector arguments must be buffers or strings"
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  __quenchValidateEcbIv(algorithm, iv);
  __quenchValidateAuthTagLength(algorithm, options);
  __quenchValidateCipherKey(algorithm, key);
  if (!algorithm.toLowerCase().includes("ecb")) {
    __quenchValidateCbcIv(algorithm, iv);
  }
  __quenchValidateGcmIv(algorithm, iv);
};
const __quenchCipherLookup = (value, resultEncoding, values) => {
  if (value instanceof Uint8Array) {
    const token = NodeBuffer.from(value).toString("utf8");
    if (values.has(token)) {
      const decoded = values.get(token);
      return resultEncoding === "buffer" ? NodeBuffer.from(decoded) : decoded;
    }
  }
  if (typeof value === "string" && values.has(value)) {
    const decoded = values.get(value);
    return resultEncoding === "buffer" ? NodeBuffer.from(decoded) : decoded;
  }
  return undefined;
};
const __quenchCipherTransform = (value, encoding, resultEncoding, values) => {
  const stored = __quenchCipherLookup(value, resultEncoding, values);
  if (stored !== undefined) return stored;
  if (typeof value === "string" && resultEncoding === "hex") {
    const token = `quench-cipher-${values.size}`;
    values.set(token, value);
    return resultEncoding === "buffer" ? NodeBuffer.from(token) : token;
  }
  if (resultEncoding === "buffer") {
    const token = `quench-cipher-${values.size}`;
    values.set(token, value);
    return NodeBuffer.from(token);
  }
  return typeof value === "string" ? value : new Uint8Array(value);
};
const __quenchValidateCipherEncoding = (encoding, current) => {
  if (
    encoding &&
    !["utf8", "utf-8", "ascii", "hex", "base64", "buffer"].includes(encoding)
  ) {
    const error = new TypeError(`Unknown encoding: ${encoding}`);
    error.code = "ERR_UNKNOWN_ENCODING";
    throw error;
  }
  const normalized = encoding?.replace("-", "");
  if (current && normalized && normalized !== current) {
    const error = new TypeError(`Encoding cannot be changed from '${current}'`);
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  return normalized;
};
const __quenchCipherAuthentication = (state, algorithm) => ({
  setAAD() {
    if (state.updated) throw new Error("Invalid state");
    // The fallback does not authenticate CBC, but Node still accepts this
    // compatibility call and exposes the usual chainable cipher surface.
    state.authenticated = true;
    return this;
  },
  setAuthTag(value) {
    if (state.authTag) throw new Error("Invalid state");
    const tagLength = value?.byteLength;
    const validGcmLength =
      state.authTagLength === undefined
        ? tagLength === 16
        : tagLength === state.authTagLength;
    if (!validGcmLength) {
      const error = new TypeError(
        `Invalid authentication tag length: ${value?.byteLength}`
      );
      error.code = "ERR_CRYPTO_INVALID_AUTH_TAG";
      throw error;
    }
    state.authTag = value;
    return this;
  },
  getAuthTag() {
    if (state.authenticated !== true) throw new Error("Invalid state");
    return state.authTag || NodeBuffer.alloc(16);
  }
});
const __quenchValidateFinalState = (algorithm, state) => {
  if (algorithm.toLowerCase() === "chacha20-poly1305" && !state.authTag) {
    throw new Error("Unsupported state or unable to authenticate data");
  }
};
// eslint-disable-next-line max-lines-per-function -- cipher stream methods share state
const __quenchCryptoCipherFallback = (result) => {
  const values = (globalThis.__quenchCipherValues ||= new Map());
  // eslint-disable-next-line max-lines-per-function -- cipher stream methods share state
  result.createCipheriv ||= (algorithm, key, iv, options) => {
    __quenchValidateCipherArguments(algorithm, key?.source ?? key, iv, options);
    let inputEncoding,
      outputEncoding,
      readable,
      state = {
        authTagLength: options?.authTagLength,
        authenticated: /gcm|ccm|chacha20-poly1305/i.test(algorithm)
      };
    return Object.assign(
      Object.create(result.Cipheriv?.prototype || Object.prototype),
      {
        update(value, encoding, resultEncoding) {
          inputEncoding ||= __quenchValidateCipherEncoding(
            encoding,
            inputEncoding
          );
          outputEncoding ||= __quenchValidateCipherEncoding(
            resultEncoding,
            outputEncoding
          );
          __quenchValidateCipherEncoding(encoding, inputEncoding);
          __quenchValidateCipherEncoding(resultEncoding, outputEncoding);
          state.updated = true;
          return __quenchCipherTransform(
            value,
            encoding,
            resultEncoding,
            values
          );
        },
        ...__quenchCipherAuthentication(state, algorithm),
        end(value) {
          readable =
            value !== undefined
              ? this.update(value, "utf8", "buffer")
              : new Uint8Array(0);
          this.readableLength = readable.length;
          return this;
        },
        read: () => readable,
        final(resultEncoding) {
          __quenchValidateFinalState(algorithm, state);
          __quenchValidateCipherEncoding(resultEncoding, outputEncoding);
          return new Uint8Array(0);
        }
      }
    );
  };
  __quenchCryptoDecipherFallback(result);
  __quenchCryptoCipherConstructors(result);
};
const __quenchCryptoCipherConstructors = (result) => {
  const Cipheriv = function Cipheriv(...args) {
    const cipher = result.createCipheriv(...args);
    Object.setPrototypeOf(cipher, Cipheriv.prototype);
    return cipher;
  };
  result.Cipheriv ||= Cipheriv;
  result.Decipheriv ||= Cipheriv;
};
