{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "dgram") {
        result = Object.assign({}, result);
        result.createSocket ||= () => undefined;
        result.Socket ||= function Socket() {};
        result.SocketAddress ||= function SocketAddress() {};
      }
      return result;
    };
  }
}
