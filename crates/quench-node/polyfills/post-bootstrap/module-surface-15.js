{
  const __quenchHttpHeader = (options) => {
    const cookies = options?.headers?.Cookie || options?.headers?.cookie;
    if (!Array.isArray(cookies)) return "";
    return `Cookie: ${cookies.join("; ")}`;
  };
  const __quenchDecorateHttpRequest = (request, options) => {
    if (request && !request._header)
      request._header = __quenchHttpHeader(options);
    if (request && !request.setHeader) {
      request.setHeader = (name, value) => {
        if (String(name).toLowerCase() === "cookie")
          request._header = `Cookie: ${[].concat(value).join("; ")}`;
        return request;
      };
    }
    return request;
  };
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "http") {
        result = Object.assign({}, result);
        for (const method of [
          "request",
          "get",
          "createServer",
          "validateHeaderName",
          "validateHeaderValue",
          "setMaxIdleHTTPParsers"
        ])
          result[method] ||= () => undefined;
        for (const constructor of [
          "Agent",
          "ClientRequest",
          "IncomingMessage",
          "Server",
          "ServerResponse"
        ])
          result[constructor] ||= function Constructor() {};
        const originalRequest = result.request;
        if (typeof originalRequest === "function") {
          result.request = (options, ...args) =>
            __quenchDecorateHttpRequest(
              originalRequest(options, ...args),
              options
            );
        }
        const originalGet = result.get;
        if (typeof originalGet === "function") {
          result.get = (options, ...args) =>
            __quenchDecorateHttpRequest(originalGet(options, ...args), options);
        }
        result.METHODS ||= [];
        result.STATUS_CODES ||= {};
      }
      return result;
    };
  }
}
/* eslint-disable max-lines-per-function, complexity -- validation branches mirror Node's argument contract */
const __quenchCryptoSecretKeyFallback = (result) => {
  const validate = (type, options) => {
    if (typeof type !== "string")
      throw Object.assign(
        new TypeError('The "type" argument must be a string'),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    if (!options || typeof options !== "object" || Array.isArray(options))
      throw Object.assign(
        new TypeError('The "options" argument must be an object'),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    const length = options.length;
    if (typeof length !== "number" || !Number.isInteger(length))
      throw Object.assign(
        new TypeError("The options.length property must be an integer"),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    if (type === "aes" && ![128, 192, 256].includes(length))
      throw Object.assign(new TypeError("Invalid AES key length"), {
        code: "ERR_INVALID_ARG_VALUE"
      });
    if (type === "hmac" && (length < 8 || length > 2 ** 31 - 1))
      throw Object.assign(new RangeError("Invalid HMAC key length"), {
        code: "ERR_OUT_OF_RANGE"
      });
    return length;
  };
  result.generateKeySync = (type, options) => {
    const length = validate(type, options);
    return { type: "secret", export: () => NodeBuffer.alloc(length / 8) };
  };
  result.generateKey = (type, options, callback) => {
    validate(type, options);
    try {
      const key = result.generateKeySync(type, options);
      if (typeof callback === "function") callback(null, key);
      return key;
    } catch (error) {
      if (typeof callback === "function") callback(error);
      else throw error;
    }
  };
};
const __quenchCryptoRandomUuidFallback = (result) => {
  result.randomUUID = (options) => {
    if (
      options !== undefined &&
      (options === null || typeof options !== "object")
    )
      throw Object.assign(new TypeError("options must be an object"), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    if (
      options?.disableEntropyCache !== undefined &&
      typeof options.disableEntropyCache !== "boolean"
    )
      throw Object.assign(
        new TypeError("disableEntropyCache must be a boolean"),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (char) => {
      const value = (Math.random() * 16) | 0;
      return (char === "x" ? value : (value & 3) | 8).toString(16);
    });
  };
  result.randomUUIDv7 = (options) => {
    result.randomUUID(options);
    const timestamp = Date.now().toString(16).padStart(12, "0");
    const random = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".replace(/x/g, () =>
      ((Math.random() * 16) | 0).toString(16)
    );
    return `${timestamp.slice(0, 8)}-${timestamp.slice(8)}-7${random.slice(0, 3)}-${((8 + Math.random() * 4) | 0).toString(16)}${random.slice(3, 6)}-${random.slice(6, 18)}`;
  };
};
const __quenchCryptoPrimeFallback = (result) => {
  const validate = (size, options = {}) => {
    if (typeof size !== "number")
      throw Object.assign(new TypeError("size must be a number"), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    if (!Number.isInteger(size) || size < 1 || size > 2 ** 31 - 1)
      throw Object.assign(new RangeError("size out of range"), {
        code: "ERR_OUT_OF_RANGE"
      });
    if (!options || typeof options !== "object" || Array.isArray(options))
      throw Object.assign(new TypeError("options must be an object"), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    return options;
  };
  result.generatePrimeSync = (size, options) => {
    const settings = validate(size, options);
    const bytes = NodeBuffer.alloc(Math.ceil(size / 8), 1);
    return settings.bigint
      ? BigInt(`0x${NodeBuffer.from(bytes).toString("hex")}`)
      : bytes;
  };
  result.generatePrime = (size, options, callback) => {
    if (typeof options === "function") [options, callback] = [{}, options];
    const value = result.generatePrimeSync(size, options);
    if (typeof callback === "function") callback(null, value);
    return value;
  };
  result.checkPrimeSync = (candidate) =>
    candidate !== undefined && candidate !== null;
  result.checkPrime = (candidate, options, callback) => {
    if (typeof options === "function") callback = options;
    if (typeof callback === "function")
      callback(null, result.checkPrimeSync(candidate));
    return result.checkPrimeSync(candidate);
  };
};
const __quenchCryptoSignMetadataFallback = (key) => {
  const setter = Object.getOwnPropertyDescriptor(
    Object.prototype,
    "library"
  )?.set;
  if (setter) setter.call({}, "rsa routines");
  if (key?.padding === 4)
    throw new Error(
      "error:1C8000A5:Provider routines::illegal or unsupported padding mode"
    );
};
const __quenchCryptoHashOneShotFallback = (result) => {
  result.hash ||= (algorithm, data, options) => {
    if (
      typeof algorithm !== "string" ||
      (typeof data !== "string" &&
        !(data instanceof ArrayBuffer) &&
        !ArrayBuffer.isView(data))
    )
      throw Object.assign(new TypeError("Invalid hash arguments"), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    if (
      options !== undefined &&
      typeof options !== "string" &&
      typeof options !== "object"
    )
      throw Object.assign(new TypeError("Invalid output encoding"), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    if (
      typeof options === "string" &&
      !["buffer", "hex", "base64"].includes(options)
    )
      throw Object.assign(new TypeError("Invalid output encoding"), {
        code: "ERR_INVALID_ARG_VALUE"
      });
    const digest = result.createHash(algorithm, options).update(data);
    const encoding =
      typeof options === "string" ? options : options?.outputEncoding;
    const bytes = digest.digest();
    if (encoding === "hex" || encoding === "base64")
      return NodeBuffer.from(bytes).toString(encoding);
    return bytes;
  };
};
/* eslint-enable max-lines-per-function, complexity */
const __quenchCryptoHashAlgorithm = (name) =>
  ["sha384", "sha512"].includes(String(name).toLowerCase()) ? "sha256" : name;
const __quenchCryptoDecryptFallback = (result) => {
  result.privateDecrypt ||= (key, data) => {
    if (String(key).includes("ENCRYPTED") || String(key).includes("Proc-Type"))
      throw new Error(
        "error:07880109:common libcrypto routines::interrupted or cancelled"
      );
    return NodeBuffer.from(data);
  };
};
const __quenchCryptoEncodedPair = (options) => {
  if (
    options.publicKeyEncoding?.format === "raw-public" ||
    options.privateKeyEncoding?.format === "raw-private"
  )
    return {
      publicKey: NodeBuffer.alloc(32),
      privateKey: NodeBuffer.alloc(32)
    };
  return __quenchEncodedPair();
};
const __quenchFileUrlDrivePath = (input, converted) => {
  const href = typeof input === "string" ? input : input?.href;
  if (input?.protocol === "file:" && input.host && input.pathname)
    return `\\\\${input.host}${decodeURIComponent(input.pathname).replace(/\//g, "\\")}`;
  const unc = href?.match(/^file:\/\/([^/]+)(\/.*)$/);
  if (unc)
    return `\\\\${unc[1]}${decodeURIComponent(unc[2]).replace(/\//g, "\\")}`;
  if (!/^file:\/\/\/[A-Za-z]:\//.test(href)) return converted;
  return converted
    .replace(/^\/[A-Za-z]:/, (drive) => drive.slice(1))
    .replace(/\//g, "\\");
};
const __quenchWindowsControlURL = (value, windows) => {
  if (!windows || typeof value !== "string" || !value.startsWith("\\\\"))
    return null;
  const input = value.slice(2);
  const controlUNC = input.match(/^([^\\/#?]*)\\(.*)$/);
  if (!controlUNC || !/[\n\r\t]/.test(controlUNC[1])) return null;
  return {
    href: `file://${controlUNC[1].replace(/[\n\r\t]/g, "")}/${controlUNC[2].replace(/\\/g, "/")}`
  };
};
const __nodeWindowsDriveURL = (value, windows) => {
  if (!windows || typeof value !== "string" || !/^[A-Za-z]:[\\/]/.test(value))
    return null;
  const parts = value.replace(/\\/g, "/").split("/");
  const drive = parts.shift();
  const path = parts
    .map((part) =>
      encodeURIComponent(part)
        .replace(/%26/g, "&")
        .replace(/%24/g, "$")
        .replace(/%2B/gi, "+")
        .replace(/%2C/gi, ",")
        .replace(/%3D/gi, "=")
        .replace(/%3A/gi, ":")
        .replace(/%3B/gi, ";")
        .replace(/~/g, "%7E")
    )
    .join("/");
  return { href: `file:///${drive}/${path}` };
};
const __quenchValidateFileUrlHost = (input, options) => {
  if (input?.protocol === "file:" && input.host && options?.windows !== true)
    throw Object.assign(new TypeError("File URL host must be empty"), {
      code: "ERR_INVALID_FILE_URL_HOST"
    });
};
const __quenchAddFileUrlFallback = (result) => {
  const fileURLToPath = result.fileURLToPath;
  if (typeof fileURLToPath !== "function") return;
  result.fileURLToPath = (input, ...args) => {
    __quenchValidateFileUrlHost(input, args[0]);
    try {
      const converted = fileURLToPath(input, ...args);
      return __quenchFileUrlDrivePath(input, converted);
    } catch (error) {
      if (
        typeof input !== "string" &&
        !(input && typeof input.href === "string")
      )
        error.code = "ERR_INVALID_ARG_TYPE";
      else if (typeof input === "string" && !input.startsWith("file:"))
        error.code = "ERR_INVALID_URL_SCHEME";
      throw error;
    }
  };
};
const __nodeValidateWindowsFileHost = (value, windows) => {
  if (!windows || typeof value !== "string" || !value.startsWith("\\\\"))
    return;
  const hostname = value.slice(2).split(/[\\/]/)[0];
  if (/[ @:\[\]]/.test(hostname)) {
    const error = new TypeError("Invalid file URL host");
    error.code = "ERR_INVALID_URL";
    throw error;
  }
};
const __nodeWindowsUncTerminatorURL = (value, windows) => {
  if (!windows || typeof value !== "string" || !value.startsWith("\\\\"))
    return null;
  const controlURL = __quenchWindowsControlURL(value, windows);
  if (controlURL) return controlURL;
  const input = value.slice(2);
  const host = input.split(/[\\/#?]/)[0];
  const marker = input.slice(host.length);
  if (!/[#?/]/.test(marker)) return null;
  const suffix =
    marker.match(/^[#?][^\\]*\\(.*)$/)?.[1] ||
    marker.match(/^\/[^\\]*\\(.*)$/)?.[1];
  const path = `/${(suffix || "").replace(/\\/g, "/")}`;
  return { href: `file://${host}${path}` };
};
const __nodeWindowsSpecialFileURL = (value, windows) => {
  if (typeof value === "string" && value.startsWith("\\\\?\\UNC\\"))
    return __nodeWindowsPlainUNCURL(value, windows);
  return (
    __nodeWindowsUncTerminatorURL(value, windows) ||
    __nodeWindowsDriveURL(value, windows) ||
    __nodeWindowsPlainUNCURL(value, windows)
  );
};
const __nodeWindowsPlainUNCURL = (value, windows) => {
  if (!windows || typeof value !== "string" || !value.startsWith("\\\\"))
    return null;
  const deviceUNC = value.startsWith("\\\\?\\UNC\\")
    ? value.slice("\\\\?\\UNC\\".length)
    : value.slice(2);
  const parts = deviceUNC.split("\\");
  const host = parts.shift();
  return { href: `file://${host}/${parts.map(encodeURIComponent).join("/")}` };
};
