//! Polyfill: `module-surface-19`

pub const JS: &str = r#"{
  const __quenchSurfaceName = (name) => String(name).replace(/^node:/, "");
  const __quenchSurfaceTlsDefaults = (result) => {
    result.connect ||= () => undefined;
    result.createServer ||= () => undefined;
    result.createSecureContext ||= () => ({});
    result.getCiphers ||= () => [];
    result.checkServerIdentity ||= () => undefined;
    result.DEFAULT_MIN_VERSION ||= "TLSv1.2";
    return result;
  };
  const __quenchSurfaceTlsConstructors = (result) => {
    for (const constructor of ["Server", "TLSSocket", "SecureContext"]) {
      result[constructor] ||= function Constructor() {};
    }
    return result;
  };
  const __quenchSurfaceTls = (result) => {
    result = Object.assign({}, result);
    if (globalThis.__nodeTlsModule) {
      result.connect = globalThis.__nodeTlsModule.connect;
      result.createServer = globalThis.__nodeTlsModule.createServer;
      result.createSecureContext =
        globalThis.__nodeTlsModule.createSecureContext;
    }
    if (globalThis.__nodeTlsModule?.convertALPNProtocols) {
      result.convertALPNProtocols =
        globalThis.__nodeTlsModule.convertALPNProtocols;
    }
    return __quenchSurfaceTlsConstructors(__quenchSurfaceTlsDefaults(result));
  };
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const normalized = __quenchSurfaceName(name);
      if (
        normalized === "diagnostics_channel" &&
        globalThis.__nodeDiagnosticsChannel
      ) {
        return globalThis.__nodeDiagnosticsChannel;
      }
      let result = originalRequire(name);
      if (
        normalized === "diagnostics_channel" &&
        globalThis.__nodeDiagnosticsChannel
      ) {
        result = globalThis.__nodeDiagnosticsChannel;
      }
      return normalized === "tls" ? __quenchSurfaceTls(result) : result;
    };
  }
}
"#;
