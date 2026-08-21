//! Polyfill: `write-validation`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeFsValidateWrite = (fd, buffer, callback) => {
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof fd !== "number") {
    throw Object.assign(new TypeError('The "fd" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!(typeof buffer === "string" || buffer instanceof Uint8Array)) {
    throw Object.assign(new TypeError('The "buffer" argument must be of type string or an instance of Buffer'), { code: "ERR_INVALID_ARG_TYPE" });
  }
};
const __nodeFsValidateWritev = (fd, buffers, callback) => {
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof fd !== "number") {
    throw Object.assign(new TypeError('The "fd" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (
    !Array.isArray(buffers) ||
    buffers.some((buffer) => !(buffer instanceof Uint8Array))
  ) {
    throw Object.assign(new TypeError('The "buffers" argument must be an array of Buffer or Uint8Array'), { code: "ERR_INVALID_ARG_TYPE" });
  }
};
"#);
