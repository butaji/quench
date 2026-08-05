const nodeMode = (mode) => {
  if (typeof mode !== "number" && typeof mode !== "string") {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const value = typeof mode === "string" ? parseInt(mode, 8) : Number(mode);
  if (typeof mode === "string" && !/^0?[0-7]+$/.test(mode)) {
    const error = new TypeError(`The "mode" argument is invalid: ${mode}`);
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  if (!Number.isFinite(value) || value < 0 || value > 0xffffffff) {
    const error = new RangeError(
      `The value of "mode" is out of range. It must be >= 0 && <= 4294967295. Received ${mode}`
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return value;
};
globalThis.__nodeFs.fchmodSync = (fd, mode) => {
  if (!Number.isInteger(fd) || fd < 0 || fd > 0x7fffffff) {
    const error = new RangeError('The value of "fd" is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  const value = nodeMode(mode);
  if (globalThis.__nodeFdPaths[fd])
    globalThis.__nodeFs.chmodSync(globalThis.__nodeFdPaths[fd], value);
};
globalThis.__nodeFs.fchmod = (fd, mode, callback) => {
  nodeMode(mode);
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  globalThis.__nodeFs.fchmodSync(fd, mode);
  queueMicrotask(() => {
    try {
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.lchmodSync = (value, mode) => (
  nodeMode(mode),
  globalThis.__nodeFs.chmodSync(value, mode)
);
globalThis.__nodeFs.lchmod = (value, mode, callback) => {
  if (typeof mode === "function") {
    callback = mode;
    mode = 0o666;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  globalThis.__nodeFs.lchmodSync(value, mode);
  queueMicrotask(() => callback(null));
};
