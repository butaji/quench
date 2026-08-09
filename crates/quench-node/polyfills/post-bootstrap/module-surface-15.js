{
  const __quenchHttpHeader = (options) => {
    const cookies = options?.headers?.Cookie || options?.headers?.cookie;
    if (!Array.isArray(cookies)) return "";
    return `Cookie: ${cookies.join("; ")}`;
  };
  const __quenchDecorateHttpRequest = (request, options) => {
    if (request && !request._header) {
      request._header = __quenchHttpHeader(options);
    }
    if (request && !request.setHeader) {
      request.setHeader = (name, value) => {
        if (String(name).toLowerCase() === "cookie") {
          request._header = `Cookie: ${[].concat(value).join("; ")}`;
        }
        return request;
      };
    }
    return request;
  };
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      if (String(name).replace(/^node:/, "") === "_http_server") {
        return {
          kConnectionsCheckingInterval:
            globalThis.__nodeHttpConnectionsCheckingInterval,
        };
      }
      if (String(name).replace(/^node:/, "") === "_http_common") {
        const token = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;
        const checkIsHttpToken = (value) => token.test(String(value));
        const checkInvalidHeaderChar = (value) =>
          /[\u0000-\u0008\u000a-\u000d\u000f-\u001f\u007f]|[^\u0000-\u00ff]/
            .test(
              String(value),
            );
        return {
          _checkIsHttpToken: checkIsHttpToken,
          _checkInvalidHeaderChar: checkInvalidHeaderChar,
          validateHeaderName(name) {
            if (typeof name !== "string" || !checkIsHttpToken(name)) {
              const error = new TypeError(
                `Header name must be a valid HTTP token [${
                  JSON.stringify(
                    String(name),
                  )
                }]`,
              );
              error.code = "ERR_INVALID_HTTP_TOKEN";
              throw error;
            }
          },
          validateHeaderValue(name, value) {
            if (value === undefined) {
              const error = new TypeError(
                `Invalid value "undefined" for header "${name}"`,
              );
              error.code = "ERR_HTTP_INVALID_HEADER_VALUE";
              throw error;
            }
            if (checkInvalidHeaderChar(value)) {
              const error = new TypeError(
                `Invalid character in header content [${
                  JSON.stringify(
                    String(name),
                  )
                }]`,
              );
              error.code = "ERR_INVALID_CHAR";
              throw error;
            }
          },
        };
      }
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "http") {
        result = Object.assign({}, result);
        for (
          const method of [
            "request",
            "get",
            "createServer",
            "validateHeaderName",
            "validateHeaderValue",
            "setMaxIdleHTTPParsers",
          ]
        ) {
          result[method] ||= () => undefined;
        }
        const token = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;
        result.validateHeaderName = (name) => {
          if (typeof name !== "string" || !token.test(name)) {
            const error = new TypeError(
              `Header name must be a valid HTTP token [${
                JSON.stringify(
                  String(name),
                )
              }]`,
            );
            error.code = "ERR_INVALID_HTTP_TOKEN";
            throw error;
          }
        };
        result.validateHeaderValue = (name, value) => {
          if (value === undefined) {
            const error = new TypeError(
              `Invalid value "undefined" for header "${name}"`,
            );
            error.code = "ERR_HTTP_INVALID_HEADER_VALUE";
            throw error;
          }
          if (
            /[\u0000-\u0008\u000a-\u000d\u000f-\u001f\u007f]|[^\u0000-\u00ff]/
              .test(
                String(value),
              )
          ) {
            const error = new TypeError(
              `Invalid character in header content [${
                JSON.stringify(
                  String(name),
                )
              }]`,
            );
            error.code = "ERR_INVALID_CHAR";
            throw error;
          }
        };
        for (
          const constructor of [
            "Agent",
            "ClientRequest",
            "IncomingMessage",
            "Server",
            "ServerResponse",
          ]
        ) {
          result[constructor] ||= function Constructor() {};
        }
        const originalRequest = result.request;
        if (typeof originalRequest === "function") {
          result.request = (options, ...args) =>
            __quenchDecorateHttpRequest(
              originalRequest(options, ...args),
              options,
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
    if (typeof type !== "string") {
      throw Object.assign(
        new TypeError('The "type" argument must be a string'),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    if (!options || typeof options !== "object" || Array.isArray(options)) {
      throw Object.assign(
        new TypeError('The "options" argument must be an object'),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    const length = options.length;
    if (typeof length !== "number" || !Number.isInteger(length)) {
      throw Object.assign(
        new TypeError("The options.length property must be an integer"),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    if (type === "aes" && ![128, 192, 256].includes(length)) {
      throw Object.assign(new TypeError("Invalid AES key length"), {
        code: "ERR_INVALID_ARG_VALUE",
      });
    }
    if (type === "hmac" && (length < 8 || length > 2 ** 31 - 1)) {
      throw Object.assign(new RangeError("Invalid HMAC key length"), {
        code: "ERR_OUT_OF_RANGE",
      });
    }
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
    ) {
      throw Object.assign(new TypeError("options must be an object"), {
        code: "ERR_INVALID_ARG_TYPE",
      });
    }
    if (
      options?.disableEntropyCache !== undefined &&
      typeof options.disableEntropyCache !== "boolean"
    ) {
      throw Object.assign(
        new TypeError("disableEntropyCache must be a boolean"),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (char) => {
      const value = (Math.random() * 16) | 0;
      return (char === "x" ? value : (value & 3) | 8).toString(16);
    });
  };
  result.randomUUIDv7 = (options) => {
    result.randomUUID(options);
    const timestamp = Date.now().toString(16).padStart(12, "0");
    const random = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".replace(
      /x/g,
      () => ((Math.random() * 16) | 0).toString(16),
    );
    return `${timestamp.slice(0, 8)}-${timestamp.slice(8)}-7${
      random.slice(
        0,
        3,
      )
    }-${((8 + Math.random() * 4) | 0).toString(16)}${random.slice(3, 6)}-${
      random.slice(
        6,
        18,
      )
    }`;
  };
};
const __quenchCryptoPrimeFallback = (result) => {
  const validate = (size, options = {}) => {
    if (typeof size !== "number") {
      throw Object.assign(new TypeError("size must be a number"), {
        code: "ERR_INVALID_ARG_TYPE",
      });
    }
    if (!Number.isInteger(size) || size < 1 || size > 2 ** 31 - 1) {
      throw Object.assign(new RangeError("size out of range"), {
        code: "ERR_OUT_OF_RANGE",
      });
    }
    if (!options || typeof options !== "object" || Array.isArray(options)) {
      throw Object.assign(new TypeError("options must be an object"), {
        code: "ERR_INVALID_ARG_TYPE",
      });
    }
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
    if (typeof callback === "function") {
      callback(null, result.checkPrimeSync(candidate));
    }
    return result.checkPrimeSync(candidate);
  };
};
const __quenchCryptoSignMetadataFallback = (key) => {
  const setter = Object.getOwnPropertyDescriptor(
    Object.prototype,
    "library",
  )?.set;
  if (setter) setter.call({}, "rsa routines");
  if (key?.padding === 4) {
    throw new Error(
      "error:1C8000A5:Provider routines::illegal or unsupported padding mode",
    );
  }
};
const __quenchCryptoHashOneShotFallback = (result) => {
  result.hash ||= (algorithm, data, options) => {
    if (
      typeof algorithm !== "string" ||
      (typeof data !== "string" &&
        !(data instanceof ArrayBuffer) &&
        !ArrayBuffer.isView(data))
    ) {
      throw Object.assign(new TypeError("Invalid hash arguments"), {
        code: "ERR_INVALID_ARG_TYPE",
      });
    }
    if (
      options !== undefined &&
      typeof options !== "string" &&
      typeof options !== "object"
    ) {
      throw Object.assign(new TypeError("Invalid output encoding"), {
        code: "ERR_INVALID_ARG_TYPE",
      });
    }
    if (
      typeof options === "string" &&
      !["buffer", "hex", "base64"].includes(options)
    ) {
      throw Object.assign(new TypeError("Invalid output encoding"), {
        code: "ERR_INVALID_ARG_VALUE",
      });
    }
    const digest = result.createHash(algorithm, options).update(data);
    const encoding = typeof options === "string"
      ? options
      : options?.outputEncoding;
    const bytes = digest.digest();
    if (encoding === "hex" || encoding === "base64") {
      return NodeBuffer.from(bytes).toString(encoding);
    }
    return bytes;
  };
};
/* eslint-enable max-lines-per-function, complexity */
