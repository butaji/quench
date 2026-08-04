{
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
        result.METHODS ||= [];
        result.STATUS_CODES ||= {};
      }
      return result;
    };
  }
}
