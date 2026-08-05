globalThis.__nodeFs.close = (fd, callback) => {
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.closeSync(fd);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};

globalThis.__nodeFs.open = (value, flags, mode, callback) => {
  if (typeof mode === "function") {
    callback = mode;
    mode = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.openSync(value, flags, mode));
    } catch (error) {
      callback(error);
    }
  });
};
