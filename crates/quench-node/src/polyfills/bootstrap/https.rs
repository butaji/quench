//! Polyfill: `https`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithHttps = globalThis.require;
const __quenchHttpsUnsupported = (operation) => {
  const error = new Error(`${operation} is not supported by quench-node`);
  error.code = "ERR_TLS_NOT_SUPPORTED";
  throw error;
};
const __quenchHttps = {
  request: () => __quenchHttpsUnsupported("https.request"),
  get: () => __quenchHttpsUnsupported("https.get"),
  createServer: () => __quenchHttpsUnsupported("https.createServer"),
};
const __quenchHttp = __quenchOriginalRequireWithHttps("http");
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
