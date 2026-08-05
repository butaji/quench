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
