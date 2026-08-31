//! Polyfill: the always-present global WebCrypto surface.

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchWebCryptoCopy = (value) => {
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
const __quenchGlobalCrypto = globalThis.crypto || {};
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
"#);
