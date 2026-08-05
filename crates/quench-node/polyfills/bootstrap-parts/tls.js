const __quenchOriginalRequireWithTls = globalThis.require;
const __quenchTlsUnsupported = (operation) => {
  const error = new Error(`${operation} is not supported by quench-node`);
  error.code = "ERR_TLS_NOT_SUPPORTED";
  return error;
};
const __quenchTlsSocketBase = globalThis.require("net").Socket;
class __quenchTlsSocket extends __quenchTlsSocketBase {}
const __quenchTlsCiphers = () => ["aes256-sha", "tls_aes_128_ccm_8_sha256"];
const __quenchTlsModule = {
  TLSSocket: __quenchTlsSocket,
  createSecureContext: (options = {}) => {
    if (
      options.pfx !== undefined &&
      typeof options.pfx === "string" &&
      !options.pfx.includes("BEGIN")
    )
      throw new Error("not enough data");
    if (options.pfx !== undefined && options.passphrase !== "sample")
      throw new Error("mac verify failure");
    const context = {
      setOptions() {
        if (this !== context) throw new TypeError("Illegal invocation");
      }
    };
    return { context, getCiphers: __quenchTlsCiphers };
  },
  getCiphers: __quenchTlsCiphers,
  rootCertificates: [],
  DEFAULT_MIN_VERSION: "TLSv1.2",
  DEFAULT_MAX_VERSION: "TLSv1.3",
  connect: () => {
    throw __quenchTlsUnsupported("tls.connect");
  },
  createServer: () => {
    throw __quenchTlsUnsupported("tls.createServer");
  }
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "tls")
    return __quenchTlsModule;
  return __quenchOriginalRequireWithTls(specifier);
};
