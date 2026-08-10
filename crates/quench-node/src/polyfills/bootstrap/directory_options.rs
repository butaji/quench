//! Polyfill: `directory-options`

pub const JS: &str = r#"globalThis.__validateOpendirOptions = (options) => {
  if (options === undefined) return;
  if (options === null || typeof options !== "object") {
    throw Object.assign(new TypeError('The "options" argument must be of type object'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (
    options.encoding !== undefined &&
    !NodeBuffer.isEncoding(options.encoding)
  ) {
    throw Object.assign(new TypeError(`The argument 'encoding' is invalid encoding. Received '${options.encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
  }
  const bufferSize = options.bufferSize;
  if (bufferSize === undefined) return;
  if (typeof bufferSize !== "number") {
    throw Object.assign(new TypeError('The "bufferSize" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!Number.isInteger(bufferSize) || bufferSize < 1) {
    throw Object.assign(new RangeError('The value of "bufferSize" is out of range'), { code: "ERR_OUT_OF_RANGE" });
  }
};
globalThis.__nodeFs.opendirSync = (value, options) => {
  globalThis.__validateOpendirOptions(options);
  return new globalThis.__nodeFs.Dir(nodeFsPath(value));
};
"#;
