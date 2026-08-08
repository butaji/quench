{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "net") {
        result = Object.assign({}, result);
        const createServer = result.createServer;
        result.createServer = (...args) => {
          const server = createServer?.(...args) || new result.Server();
          if (result.Server?.prototype && !(server instanceof result.Server)) {
            const currentPrototype = Object.getPrototypeOf(server);
            if (
              Object.getPrototypeOf(result.Server.prototype) !==
              currentPrototype
            ) {
              Object.setPrototypeOf(result.Server.prototype, currentPrototype);
            }
            Object.setPrototypeOf(server, result.Server.prototype);
          }
          if (server.listening === undefined) server.listening = false;
          if (server.connections === undefined) server.connections = undefined;
          if (server.maxConnections === undefined) {
            server.maxConnections = undefined;
          }
          if (typeof server.address !== "function") server.address = () => null;
          server.unref ||= () => server;
          server.ref ||= () => server;
          const listen = server.listen;
          server.listen = (...listenArgs) => {
            server.listening = true;
            const result = listen?.(...listenArgs);
            return result || server;
          };
          const close = server.close;
          server.close = (callback) => {
            server.listening = false;
            const result = close?.(callback);
            callback?.();
            return result || server;
          };
          return server;
        };
        result.createConnection ||= () => undefined;
        result.connect ||= result.createConnection;
        const originalIsIP = result.isIP;
        const originalIsIPv4 = result.isIPv4;
        const originalIsIPv6 = result.isIPv6;
        const validDottedTail = /(?:^|:)\d{1,3}(?:\.\d{1,3}){3}$/;
        const isMalformedIPv6 = (value) => {
          if (typeof value !== "string" || !value.includes(":")) return false;
          if (
            (value.startsWith(":") && !value.startsWith("::")) ||
            (value.endsWith(":") && !value.endsWith("::")) ||
            value.indexOf("::", value.indexOf("::") + 2) !== -1
          ) {
            return true;
          }
          const dotted = /:[^%]*\./.test(value);
          if (dotted && !validDottedTail.test(value)) return true;
          const zone = value.match(/%(.+)$/)?.[1];
          return Boolean(zone && !/^[0-9A-Za-z._-]+$/.test(zone));
        };
        result.isIP = (value) =>
          isMalformedIPv6(value) ? 0 : originalIsIP?.(value) || 0;
        result.isIPv4 = (value) =>
          typeof value === "string" && value.includes(":")
            ? false
            : originalIsIPv4?.(value) || false;
        result.isIPv6 = (value) =>
          isMalformedIPv6(value) ? false : originalIsIPv6?.(value) || false;
        for (const constructor of [
          "Server",
          "Socket",
          "SocketAddress",
          "BlockList"
        ]) {
          result[constructor] ||= function Constructor() {};
        }
      }
      return result;
    };
  }
}
