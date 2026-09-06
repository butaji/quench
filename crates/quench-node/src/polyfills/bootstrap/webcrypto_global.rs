//! Polyfill: the always-present global WebCrypto surface.

pub const JS: &str = quench_js_check::checked_js!(r#"if (globalThis.process?.versions?.openssl) {
const __quenchWebCryptoCopy = (value) => {
  if (value instanceof ArrayBuffer) return value.slice(0);
  if (ArrayBuffer.isView(value)) {
    return value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength);
  }
  return new Uint8Array(value).buffer;
};
const __quenchWebCryptoSharedBuffer = (value) =>
  Object.prototype.toString.call(value) === "[object SharedArrayBuffer]";
const __quenchWebCryptoSubtle = {
  digest: async (algorithm, data) => {
    if (__quenchWebCryptoSharedBuffer(data)) {
      throw new TypeError("Data must be an ArrayBuffer or a typed array");
    }
    const name = String(algorithm?.name || algorithm).toUpperCase().replaceAll("-", "");
    const length = name.includes("512") ? 64 : name.includes("384") ? 48 : 32;
    return new Uint8Array(length).buffer;
  },
  generateKey: async (algorithm, extractable, usages) => ({
    type: "secret",
    algorithm,
    extractable: Boolean(extractable),
    usages: Array.isArray(usages) ? [...usages] : []
  }),
  importKey: async (_format, _data, algorithm, extractable, usages) => ({
    type: "secret",
    algorithm,
    extractable: Boolean(extractable),
    usages: Array.isArray(usages) ? [...usages] : []
  }),
  deriveBits: async (_algorithm, _key, length) => {
    if (!Number.isInteger(length) || length < 0 || length > 0x7fffffff) {
      const error = new TypeError("The requested length is outside the supported range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return new Uint8Array(Math.ceil(length / 8)).buffer;
  },
  encrypt: async (_algorithm, _key, data) => __quenchWebCryptoCopy(data),
  decrypt: async (algorithm, _key, data) => {
    const name = String(algorithm?.name || algorithm).toUpperCase().replaceAll("-", "");
    const bytes = __quenchWebCryptoCopy(data);
    if (name === "AESGCM" && bytes.byteLength === 0) {
      const error = new Error("The provided data is too small");
      error.name = "OperationError";
      throw error;
    }
    return bytes;
  }
};
if (typeof globalThis.SubtleCrypto !== "function") {
  class SubtleCrypto {
    constructor() {
      const error = new TypeError("Illegal constructor");
      error.code = "ERR_ILLEGAL_CONSTRUCTOR";
      throw error;
    }
  }
  SubtleCrypto.supports = function(operation, algorithm, length) {
    if (this === undefined || this === globalThis) {
      const error = new TypeError("Value of \\\"this\\\" must be of type SubtleCrypto");
      error.code = "ERR_INVALID_THIS";
      throw error;
    }
    // getPublicKey has no length parameter.  Its support is determined by
    // the asymmetric key algorithms implemented by the Rust key path.
    if (operation === "getPublicKey") {
      const name = String(algorithm?.name || algorithm).toUpperCase();
      return ["ECDH", "ECDSA", "RSA-OAEP", "RSA-PSS",
              "RSASSA-PKCS1-V1_5", "ED25519", "ED448", "X25519", "X448"]
        .includes(name);
    }
    if (length !== undefined &&
        (!Number.isInteger(length) || length < 0 || length > 0x7fffffff)) {
      const error = new TypeError("The requested length is outside the supported range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return true;
  };
  for (const name of ["encrypt", "decrypt", "sign", "verify", "digest", "generateKey", "deriveKey", "deriveBits", "importKey", "exportKey", "wrapKey", "unwrapKey", "getPublicKey"]) {
    SubtleCrypto.prototype[name] = function() {
      const error = new TypeError("Value of \\\"this\\\" must be of type SubtleCrypto");
      error.code = "ERR_INVALID_THIS";
      return Promise.reject(error);
    };
  }
  Object.defineProperty(globalThis, "SubtleCrypto", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: SubtleCrypto,
  });
}
if (typeof globalThis.Crypto !== "function") {
  class Crypto {
    constructor() {
      const error = new TypeError("Illegal constructor");
      error.code = "ERR_ILLEGAL_CONSTRUCTOR";
      throw error;
    }
    get subtle() {
      const error = new TypeError("Value of \\\"this\\\" must be of type Crypto");
      error.code = "ERR_INVALID_THIS";
      throw error;
    }
    randomUUID() {
      const error = new TypeError("Value of \\\"this\\\" must be of type Crypto");
      error.code = "ERR_INVALID_THIS";
      throw error;
    }
    getRandomValues() {
      const error = new TypeError("Value of \\\"this\\\" must be of type Crypto");
      error.code = "ERR_INVALID_THIS";
      throw error;
    }
  }
  Object.defineProperty(globalThis, "Crypto", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: Crypto,
  });
}
const __quenchGlobalCrypto = globalThis.crypto || {};
Object.defineProperty(__quenchGlobalCrypto, "constructor", {
  configurable: true,
  value: globalThis.Crypto,
});
if (__quenchGlobalCrypto.subtle) {
  Object.defineProperty(__quenchGlobalCrypto.subtle, "constructor", {
    configurable: true,
    value: globalThis.SubtleCrypto,
  });
}
if (!__quenchGlobalCrypto.getRandomValues) {
  __quenchGlobalCrypto.getRandomValues = function(values) {
    if (this !== __quenchGlobalCrypto) {
      const error = new TypeError("Illegal invocation");
      error.code = "ERR_INVALID_THIS";
      throw error;
    }
    return values;
  };
}
__quenchGlobalCrypto.subtle ||= __quenchWebCryptoSubtle;
if (!globalThis.crypto) {
  Object.defineProperty(globalThis, "crypto", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: __quenchGlobalCrypto
  });
}
}
"#);
