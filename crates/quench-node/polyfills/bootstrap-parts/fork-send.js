const __quenchForkSendRequire = globalThis.require;
const __quenchForkSendModule = __quenchForkSendRequire("child_process");
const __quenchForkSendOriginal = __quenchForkSendModule.fork;
const __quenchValidateSend = (send, values) => {
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
  return send(...values);
};
__quenchForkSendModule.fork = (...args) => {
  const child = __quenchForkSendOriginal(...args);
  const send = child.send;
  child.send = (...values) => __quenchValidateSend(send, values);
  return child;
};
