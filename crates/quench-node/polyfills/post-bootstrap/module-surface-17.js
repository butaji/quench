{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "net") {
        result = Object.assign({}, result);
        result.createServer ||= () => undefined;
        result.createConnection ||= () => undefined;
        result.connect ||= result.createConnection;
        result.isIP ||= () => 0;
        result.isIPv4 ||= () => false;
        result.isIPv6 ||= () => false;
        for (const constructor of [
          "Server",
          "Socket",
          "SocketAddress",
          "BlockList"
        ])
          result[constructor] ||= function Constructor() {};
      }
      return result;
    };
  }
}
