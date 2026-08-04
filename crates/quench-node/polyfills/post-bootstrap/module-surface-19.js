{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "tls") {
        result = Object.assign({}, result);
        result.connect ||= () => undefined;
        result.createServer ||= () => undefined;
        result.createSecureContext ||= () => ({});
        result.getCiphers ||= () => [];
        result.checkServerIdentity ||= () => undefined;
        for (const constructor of ["Server", "TLSSocket", "SecureContext"])
          result[constructor] ||= function Constructor() {};
        result.DEFAULT_MIN_VERSION ||= "TLSv1.2";
      }
      return result;
    };
  }
}
