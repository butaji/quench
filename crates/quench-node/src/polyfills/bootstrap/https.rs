//! Polyfill: `https`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithHttps = globalThis.require;
const __quenchHttpsUnsupported = (operation) => {
  const error = new Error(`${operation} is not supported by quench-node`);
  error.code = "ERR_TLS_NOT_SUPPORTED";
  throw error;
};
// The HTTP implementation is transport-complete for plain HTTP. Reuse it for
// explicit `http:` URLs passed through the https facade, while keeping real
// TLS requests an explicit, stable unsupported operation.
const __quenchHttpsRequest = (options, callback) => {
  const target = typeof options === "string" ? options : options && options.href;
  if (target && String(target).startsWith("http://")) {
    return __quenchHttp.request(options, callback);
  }
  if (options && typeof options === "object" && options.protocol === "http:") {
    return __quenchHttp.request(options, callback);
  }
  return __quenchHttpsUnsupported("https.request");
};
const __quenchHttpsGet = (options, callback) => {
  const request = __quenchHttpsRequest(options, callback);
  if (request && typeof request.end === "function") request.end();
  return request;
};
const __quenchHttps = {
  request: __quenchHttpsRequest,
  get: __quenchHttpsGet,
  createServer: () => __quenchHttpsUnsupported("https.createServer"),
};
const __quenchHttp = __quenchOriginalRequireWithHttps("http");
class __quenchHttpsAgent extends (__quenchHttp.Agent || class {}) {
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
