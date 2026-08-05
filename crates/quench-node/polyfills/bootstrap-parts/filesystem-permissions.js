const nodeMode = (mode) => {
  if (typeof mode !== "number" && typeof mode !== "string") {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const value = typeof mode === "string" ? parseInt(mode, 8) : Number(mode);
  if (!Number.isFinite(value) || value < 0 || value > 0xffffffff) {
    const error = new RangeError('The value of "mode" is out of range');
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
