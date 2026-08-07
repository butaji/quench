{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      if (
        String(name).replace(/^node:/, "") === "diagnostics_channel" &&
        globalThis.__nodeDiagnosticsChannel
      ) {
        return globalThis.__nodeDiagnosticsChannel;
      }
      let result = originalRequire(name);
      if (
        String(name).replace(/^node:/, "") === "diagnostics_channel" &&
        globalThis.__nodeDiagnosticsChannel
      ) {
        result = globalThis.__nodeDiagnosticsChannel;
      }
      if (String(name).replace(/^node:/, "") === "tls") {
        result = Object.assign({}, result);
        if (globalThis.__nodeTlsModule) {
          result.connect = globalThis.__nodeTlsModule.connect;
          result.createServer = globalThis.__nodeTlsModule.createServer;
          result.createSecureContext =
            globalThis.__nodeTlsModule.createSecureContext;
        }
        result.connect ||= () => undefined;
        result.createServer ||= () => undefined;
        result.createSecureContext ||= () => ({});
        result.getCiphers ||= () => [];
        if (globalThis.__nodeTlsModule?.convertALPNProtocols) {
          result.convertALPNProtocols =
            globalThis.__nodeTlsModule.convertALPNProtocols;
        }
        result.checkServerIdentity ||= () => undefined;
        for (const constructor of ["Server", "TLSSocket", "SecureContext"]) {
          result[constructor] ||= function Constructor() {};
        }
        result.DEFAULT_MIN_VERSION ||= "TLSv1.2";
      }
      return result;
    };
  }
}
