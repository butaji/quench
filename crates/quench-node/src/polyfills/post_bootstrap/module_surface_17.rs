//! Polyfill: `module-surface-17`

pub const JS: &str = quench_js_check::checked_js!(r#"{
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
            const result = close?.(callback);
            if (!close) {
              server.listening = false;
              server.emit?.("close");
              callback?.call(server);
            }
            return result || server;
          };
          return server;
        };
        result.createConnection ||= () => undefined;
        result.connect ||= result.createConnection;
        const originalIsIP = result.isIP;
        const originalIsIPv4 = result.isIPv4;
        const originalIsIPv6 = result.isIPv6;
        const validDottedTail = (value) => {
          const tail = value.match(/(?:^|:)(\d+(?:\.\d+)*)$/)?.[1];
          if (!tail) return false;
          const parts = tail.split(".");
          return (
            parts.length === 4 &&
            parts.every((part) => {
              const number = Number(part);
              return (
                /^\d+$/.test(part) &&
                part.length <= 3 &&
                (part.length === 1 || !part.startsWith("0")) &&
                number <= 255
              );
            })
          );
        };
        const strictIPv6 = (value) => {
          if (typeof value !== "string" || !value.includes(":")) return false;
          const address = value.split("%")[0];
          if (
            !address ||
            (value.includes("%") &&
              !/^[0-9A-Za-z._-]+$/.test(value.split("%")[1]))
          ) {
            return false;
          }
          const marker = address.indexOf("::");
          if (marker !== -1 && address.indexOf("::", marker + 2) !== -1) {
            return false;
          }
          const hasCompression = marker !== -1;
          const head = hasCompression ? address.slice(0, marker) : address;
          const tail = hasCompression ? address.slice(marker + 2) : "";
          const groups = (part) => (part ? part.split(":") : []);
          const headGroups = groups(head);
          const tailGroups = groups(tail);
          if (
            hasCompression &&
            headGroups.some((group) => group.includes("."))
          ) {
            return false;
          }
          const all = [...headGroups, ...tailGroups];
          if (
            all.some((group, index) =>
              group.includes(".")
                ? index !== all.length - 1 || !validDottedTail(`::${group}`)
                : !/^[0-9a-fA-F]{1,4}$/.test(group)
            )
          ) {
            return false;
          }
          const width = all.reduce(
            (count, group) => count + (group.includes(".") ? 2 : 1),
            0
          );
          return hasCompression ? width < 8 : width === 8;
        };
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
          if (dotted && !validDottedTail(value)) return true;
          const zone = value.match(/%(.+)$/)?.[1];
          return Boolean(zone && !/^[0-9A-Za-z._-]+$/.test(zone));
        };
        result.isIP = (value) =>
          typeof value === "string" && value.includes(":")
            ? strictIPv6(value)
              ? 6
              : 0
            : originalIsIP?.(value) || 0;
        result.isIPv4 = (value) =>
          typeof value === "string" && value.includes(":")
            ? false
            : originalIsIPv4?.(value) || false;
        result.isIPv6 = (value) =>
          typeof value === "string" && value.includes(":")
            ? strictIPv6(value)
            : originalIsIPv6?.(value) || false;
        for (const constructor of "Server Socket SocketAddress BlockList".split(
          " "
        )) {
          result[constructor] ||= function Constructor() {};
        }
      }
      return result;
    };
  }
}
"#);
