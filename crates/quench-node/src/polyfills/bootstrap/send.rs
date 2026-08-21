//! Polyfill: `send`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchProcessSend = globalThis.process;
const __quenchOriginalProcessSend = __quenchProcessSend.send;
__quenchProcessSend.send = (...values) => {
  if (values.length > 3) {
    throw Object.assign(new TypeError("The callback argument must be a function"), { code: "ERR_INVALID_ARG_TYPE" });
  }
  const handle = values[1];
  const callback = values[2];
  if (handle !== null && handle !== undefined && typeof handle !== "object") {
    throw Object.assign(new TypeError("This handle type cannot be sent"), { code: "ERR_INVALID_HANDLE_TYPE" });
  }
  if (
    values.length >= 3 &&
    callback !== undefined &&
    typeof callback !== "function"
  ) {
    throw Object.assign(new TypeError("The callback argument must be a function"), { code: "ERR_INVALID_ARG_TYPE" });
  }
  return __quenchOriginalProcessSend(...values);
};
"#);
