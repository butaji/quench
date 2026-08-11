//! Polyfill: `filesystem-access-validation`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeFsValidateAccessMode = (mode) => {
  if (mode === undefined) return;
  if (typeof mode !== "number") {
    throw Object.assign(new TypeError('The "mode" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (
    !Number.isFinite(mode) ||
    !Number.isInteger(mode) ||
    mode < 0 ||
    mode > 7
  ) {
    throw Object.assign(new RangeError('The value of "mode" is out of range'), { code: "ERR_OUT_OF_RANGE" });
  }
};
globalThis.__nodeFsExists = (value, callback) => {
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  let exists = false;
  try {
    exists = globalThis.__quench_fs_exists(nodePathValue(value));
  } catch (_) {}
  callback(exists);
};
globalThis.__nodeFs.exists = globalThis.__nodeFsExists;
globalThis.__nodeFs.existsSync = (value) => {
  try {
    return Boolean(globalThis.__quench_fs_exists(nodePathValue(value)));
  } catch (_) {
    return false;
  }
};
"#);
