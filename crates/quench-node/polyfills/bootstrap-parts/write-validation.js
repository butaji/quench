const __nodeFsValidateWrite = (fd, buffer, callback) => {
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!(typeof buffer === "string" || buffer instanceof Uint8Array)) {
    const error = new TypeError(
      'The "buffer" argument must be of type string or an instance of Buffer'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __nodeFsValidateWritev = (fd, buffers, callback) => {
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    !Array.isArray(buffers) ||
    buffers.some((buffer) => !(buffer instanceof Uint8Array))
  ) {
    const error = new TypeError(
      'The "buffers" argument must be an array of Buffer or Uint8Array'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
