//! Polyfill: `filesystem-internals`

pub const JS: &str = r#"const __quenchRmValidateBoolean = (options, key) => {
  if (
    !Object.prototype.hasOwnProperty.call(options, key) ||
    typeof options[key] === "boolean"
  ) {
    return;
  }
  throw Object.assign(new TypeError(`The "options.${key}" property must be of type boolean.`), { code: "ERR_INVALID_ARG_TYPE" });
};
const __quenchRmValidateRange = (options, key) => {
  if (
    options[key] === undefined ||
    (Number.isFinite(options[key]) && options[key] >= 0)
  ) {
    return;
  }
  throw Object.assign(new RangeError(`The value of "options.${key}" is out of range.`), { code: "ERR_OUT_OF_RANGE" });
};
const __quenchInternalFsUtilsModule = {
  vfsState: globalThis.__quenchVfsState ||= { handlers: null },
  validateRmOptionsSync: function (_path, options) {
    if (
      arguments.length > 1 &&
      (options === null || typeof options !== "object")
    ) {
      throw Object.assign(new TypeError('The "options" argument must be of type object.'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    const value = options || {};
    __quenchRmValidateBoolean(value, "recursive");
    __quenchRmValidateBoolean(value, "force");
    __quenchRmValidateRange(value, "retryDelay");
    __quenchRmValidateRange(value, "maxRetries");
    return {
      retryDelay: value.retryDelay === undefined ? 100 : value.retryDelay,
      maxRetries: value.maxRetries === undefined ? 0 : value.maxRetries,
      recursive: value.recursive === true,
      force: value.force === true,
    };
  },
  stringToFlags: (flags) => {
    const values = {
      r: 0,
      "r+": 2,
      rs: 1052674,
      "rs+": 1052674,
      sr: 1052674,
      "sr+": 1052674,
      w: 577,
      "w+": 578,
      wx: 705,
      xw: 705,
      "wx+": 706,
      "xw+": 706,
      a: 1089,
      "a+": 1090,
      ax: 1217,
      xa: 1217,
      "ax+": 1218,
      as: 1053761,
      sa: 1053761,
      "as+": 1053762,
      "sa+": 1053762,
    };
    if (typeof flags !== "string" || values[flags] === undefined) {
      throw Object.assign(new TypeError(`Unknown file open flag: ${flags}`), { code: "ERR_INVALID_ARG_VALUE" });
    }
    return values[flags];
  },
};
"#;
