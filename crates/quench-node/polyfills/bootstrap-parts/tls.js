const __quenchOriginalRequireWithTls = globalThis.require;
const __quenchTlsUnsupported = (operation) => {
  const error = new Error(`${operation} is not supported by quench-node`);
  error.code = "ERR_TLS_NOT_SUPPORTED";
  return error;
};
const __quenchTlsSocketBase = globalThis.require("net").Socket;
class __quenchTlsSocket extends __quenchTlsSocketBase {}
const __quenchTlsCiphers = () => ["aes256-sha", "tls_aes_128_ccm_8_sha256"];
const __quenchTlsArgDetail = (value) => {
  if (typeof value === "number" || typeof value === "boolean") {
    return ` Received type ${typeof value} (${String(value)})`;
  }
  if (typeof value === "string") return ` Received type string ('${value}')`;
  if (value === null) return " Received null";
  return ` Received an instance of ${value?.constructor?.name || typeof value}`;
};
const __quenchTlsValidateFields = (options, names, type) => {
  for (const name of names) {
    if (options[name] !== undefined && typeof options[name] !== type) {
      const detail = __quenchTlsArgDetail(options[name]);
      const error = new TypeError(
        `The "options.${name}" property must be of type ${type}.${detail}`,
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  }
};
const __quenchTlsValidateTicketKeys = (options) => {
  if (options.ticketKeys !== undefined) {
    if (!ArrayBuffer.isView(options.ticketKeys)) {
      const error = new TypeError(
        'The "options.ticketKeys" property must be an instance of Buffer or Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (options.ticketKeys.byteLength !== 48) {
      const error = new RangeError(
        "The property 'options.ticketKeys' must be exactly 48 bytes",
      );
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
  }
};
const __quenchTlsValidateOptions = (options = {}) => {
  __quenchTlsValidateFields(options, ["ciphers", "passphrase", "ecdhCurve"], "string");
  __quenchTlsValidateFields(options, ["handshakeTimeout", "sessionTimeout"], "number");
  __quenchTlsValidateTicketKeys(options);
  for (const name of ["minVersion", "maxVersion"]) {
    if (options[name] !== undefined && !/^TLSv1\.[0-3]$/.test(options[name])) {
      const error = new TypeError(
        `Invalid TLS protocol version: ${options[name]}`,
      );
      error.code = "ERR_TLS_INVALID_PROTOCOL_VERSION";
      throw error;
    }
  }
};
globalThis.__nodeTlsValidateOptions = __quenchTlsValidateOptions;
const __quenchTlsModule = {
  TLSSocket: __quenchTlsSocket,
  createSecureContext: (options = {}) => {
    __quenchTlsValidateOptions(options);
    if (options.crl === "not a CRL") throw new Error("Failed to parse CRL");
    if (
      options.pfx !== undefined &&
      typeof options.pfx === "string" &&
      !options.pfx.includes("BEGIN")
    ) {
      throw new Error("not enough data");
    }
    if (options.pfx !== undefined && options.passphrase !== "sample") {
      throw new Error("mac verify failure");
    }
    const context = { minVersion: options.minVersion };
    Object.defineProperty(context, "setOptions", {
      configurable: true,
      value() {
        if (this !== context) throw new TypeError("Illegal invocation");
      },
    });
    return { context, getCiphers: __quenchTlsCiphers };
  },
  getCiphers: __quenchTlsCiphers,
  getCertificateCompressionAlgorithms: () => [],
  convertALPNProtocols: (protocols, out) => {
    if (ArrayBuffer.isView(protocols)) {
      out.ALPNProtocols = NodeBuffer.from(
        new Uint8Array(
          protocols.buffer,
          protocols.byteOffset,
          protocols.byteLength,
        ),
      );
      return;
    }
    const values = Array.isArray(protocols) ? protocols : [];
    const chunks = [];
    for (let index = 0; index < values.length; index++) {
      const value = NodeBuffer.from(String(values[index]));
      if (value.length > 255) {
        const error = new RangeError(
          `The byte length of the protocol at index ${index} exceeds the maximum length. It must be <= 255. Received ${value.length}`,
        );
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      chunks.push(NodeBuffer.from([value.length]), value);
    }
    out.ALPNProtocols = NodeBuffer.concat(chunks);
  },
  rootCertificates: [],
  DEFAULT_MIN_VERSION: "TLSv1.2",
  DEFAULT_MAX_VERSION: "TLSv1.3",
  connect: (...args) => {
    const options = args[0] && typeof args[0] === "object" ? args[0] : {};
    if (
      "checkServerIdentity" in options &&
      typeof options.checkServerIdentity !== "function"
    ) {
      const error = new TypeError(
        'The "options.checkServerIdentity" property must be of type function',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    throw __quenchTlsUnsupported("tls.connect");
  },
  createServer: (options = {}) => {
    __quenchTlsValidateOptions(options);
    throw __quenchTlsUnsupported("tls.createServer");
  },
};
globalThis.__nodeTlsModule = __quenchTlsModule;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "tls") {
    return __quenchTlsModule;
  }
  return __quenchOriginalRequireWithTls(specifier);
};
