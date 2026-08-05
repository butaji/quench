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
