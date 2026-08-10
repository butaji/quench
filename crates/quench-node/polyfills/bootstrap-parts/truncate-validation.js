globalThis.__validateTruncateLength = (length) => {
  if (typeof length !== "number" || !Number.isFinite(length)) {
    const error = new TypeError(
      `The "len" argument must be of type number.${
        __nodeInvalidArgSuffix(
          length,
        )
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(length)) {
    const error = new RangeError(
      `The value of "len" is out of range. It must be an integer. Received ${length}`,
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
globalThis.__truncateMissingPath = (path, callback) => {
  if (globalThis.__quench_fs_access(path)) return false;
  const error = new Error(`ENOENT: no such file or directory, open '${path}'`);
  error.code = "ENOENT";
  error.path = path;
  error.syscall = "open";
  queueMicrotask(() => callback(error));
  return true;
};
