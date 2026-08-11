//! Polyfill: `open-validation`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeFsValidateMode = (mode) => {
  if (
    mode !== undefined &&
    mode !== null &&
    typeof mode !== "number" &&
    typeof mode !== "string"
  ) {
    throw Object.assign(new TypeError('The "mode" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof mode === "string" && !/^0?[0-7]+$/.test(mode)) {
    throw Object.assign(new TypeError(`The "mode" argument is invalid: ${mode}`), { code: "ERR_INVALID_ARG_VALUE" });
  }
};
"#);
