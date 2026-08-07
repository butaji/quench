const __nodeFsValidateAccessMode = (mode) => {
  if (mode === undefined) return;
  if (typeof mode !== "number") {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    !Number.isFinite(mode) ||
    !Number.isInteger(mode) ||
    mode < 0 ||
    mode > 7
  ) {
    const error = new RangeError('The value of "mode" is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
globalThis.__nodeFsExists = (value, callback) => {
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
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
