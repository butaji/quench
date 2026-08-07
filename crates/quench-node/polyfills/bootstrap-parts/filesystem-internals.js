const __quenchRmValidateBoolean = (options, key) => {
  if (
    !Object.prototype.hasOwnProperty.call(options, key) ||
    typeof options[key] === "boolean"
  ) {
    return;
  }
  const error = new TypeError(
    `The "options.${key}" property must be of type boolean.`,
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
const __quenchRmValidateRange = (options, key) => {
  if (
    options[key] === undefined ||
    (Number.isFinite(options[key]) && options[key] >= 0)
  ) {
    return;
  }
  const error = new RangeError(
    `The value of "options.${key}" is out of range.`,
  );
  error.code = "ERR_OUT_OF_RANGE";
  throw error;
};
const __quenchInternalFsUtilsModule = {
  validateRmOptionsSync: function (_path, options) {
    if (
      arguments.length > 1 &&
      (options === null || typeof options !== "object")
    ) {
      const error = new TypeError(
        'The "options" argument must be of type object.',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
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
      const error = new TypeError(`Unknown file open flag: ${flags}`);
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    return values[flags];
  },
};
