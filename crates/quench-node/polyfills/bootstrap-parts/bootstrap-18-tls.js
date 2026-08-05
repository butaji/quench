const __quenchOriginalRequireWithTls = globalThis.require;
const __quenchTlsUnsupported = (operation) => {
  const error = new Error(`${operation} is not supported by quench-node`);
  error.code = "ERR_TLS_NOT_SUPPORTED";
  return error;
};
const __quenchTlsModule = {
  createSecureContext: (options = {}) => {
    if (options.pfx !== undefined && options.passphrase !== "sample")
      throw new Error("mac verify failure");
    const context = {
      setOptions() {
        if (this !== context) throw new TypeError("Illegal invocation");
      }
    };
    return { context, getCiphers: () => [] };
  },
  getCiphers: () => [],
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
