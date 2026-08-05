const __nodeFsValidateMode = (mode) => {
  if (
    mode !== undefined &&
    mode !== null &&
    typeof mode !== "number" &&
    typeof mode !== "string"
  ) {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof mode === "string" && !/^0?[0-7]+$/.test(mode)) {
    const error = new TypeError(`The "mode" argument is invalid: ${mode}`);
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
};
