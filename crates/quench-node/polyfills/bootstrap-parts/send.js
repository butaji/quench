const __quenchProcessSend = globalThis.process;
const __quenchOriginalProcessSend = __quenchProcessSend.send;
__quenchProcessSend.send = (...values) => {
  if (values.length > 3) {
    const error = new TypeError("The callback argument must be a function");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const handle = values[1];
  const callback = values[2];
  if (handle !== null && handle !== undefined && typeof handle !== "object") {
    const error = new TypeError("This handle type cannot be sent");
    error.code = "ERR_INVALID_HANDLE_TYPE";
    throw error;
  }
  if (
    values.length >= 3 &&
    callback !== undefined &&
    typeof callback !== "function"
  ) {
    const error = new TypeError("The callback argument must be a function");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  return __quenchOriginalProcessSend(...values);
};
