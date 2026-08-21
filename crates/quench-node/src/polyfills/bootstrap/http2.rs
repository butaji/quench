//! Polyfill: `http2`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithHttp2 = globalThis.require;
const __quenchHttp2Unsupported = (operation) => {
  const error = new Error(`${operation} is not supported by quench-node`);
  error.code = "ERR_HTTP2_NOT_SUPPORTED";
  throw error;
};
const __quenchHttp2 = {
  createServer: () => __quenchHttp2Unsupported("http2.createServer"),
  connect: () => __quenchHttp2Unsupported("http2.connect"),
  constants: Object.freeze({
    NGHTTP2_SESSION_SERVER: 0x01,
    NGHTTP2_SESSION_CLIENT: 0x02,
  }),
};
globalThis.require = (specifier) =>
  String(specifier).replace(/^node:/, "") === "http2"
    ? __quenchHttp2
    : __quenchOriginalRequireWithHttp2(specifier);
"#);
