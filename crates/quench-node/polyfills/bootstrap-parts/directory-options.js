globalThis.__validateOpendirOptions = (options) => {
  if (options === undefined) return;
  if (options === null || typeof options !== "object") {
    const error = new TypeError(
      'The "options" argument must be of type object'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const bufferSize = options.bufferSize;
  if (bufferSize === undefined) return;
  if (typeof bufferSize !== "number") {
    const error = new TypeError(
      'The "bufferSize" argument must be of type number'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(bufferSize) || bufferSize < 1) {
    const error = new RangeError('The value of "bufferSize" is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
globalThis.__nodeFs.opendirSync = (value, options) => {
  globalThis.__validateOpendirOptions(options);
  return new globalThis.__nodeFs.Dir(nodeFsPath(value));
};
