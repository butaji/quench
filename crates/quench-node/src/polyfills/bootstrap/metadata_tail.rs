//! Polyfill: `metadata-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"globalThis.__nodeFs.readlink = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const encoding = typeof options === "string"
    ? options
    : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    throw Object.assign(new TypeError(`The argument 'encoding' is invalid. Received '${encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.readlinkSync(path, options));
    } catch (error) {
      callback(error);
    }
  });
};
"#);
