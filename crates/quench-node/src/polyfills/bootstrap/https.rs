//! Polyfill: `https`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithHttps = globalThis.require;
const __quenchHttp = __quenchOriginalRequireWithHttps("http");
// QuenchRuntime's transport is loopback HTTP/1.1. Reuse its complete
// request/server contract for HTTPS spellings until a TLS backend exists;
// callers still receive the normal Agent defaults for port/protocol.
const __quenchHttps = {
  request: __quenchHttp.request,
  get: __quenchHttp.get,
  createServer: __quenchHttp.createServer,
};
class __quenchHttpsAgent extends __quenchHttp.Agent {
  constructor(options = {}) {
    super(options);
    this.defaultPort = 443;
    this.protocol = "https:";
  }
}
__quenchHttps.Agent = __quenchHttpsAgent;
__quenchHttps.globalAgent = new __quenchHttpsAgent({ keepAlive: true });
globalThis.require = (specifier) =>
  String(specifier).replace(/^node:/, "") === "https"
    ? __quenchHttps
    : __quenchOriginalRequireWithHttps(specifier);
"#);
