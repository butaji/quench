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
};
/* eslint-enable max-lines-per-function, complexity */
