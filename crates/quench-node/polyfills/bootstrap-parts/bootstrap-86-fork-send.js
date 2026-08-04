const __quenchForkSendRequire = globalThis.require;
const __quenchForkSendModule = __quenchForkSendRequire("child_process");
const __quenchForkSendOriginal = __quenchForkSendModule.fork;
const __quenchValidateSend = (send, values) => {
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
  return send(...values);
};
__quenchForkSendModule.fork = (...args) => {
  const child = __quenchForkSendOriginal(...args);
  const send = child.send;
  child.send = (...values) => __quenchValidateSend(send, values);
  return child;
};
