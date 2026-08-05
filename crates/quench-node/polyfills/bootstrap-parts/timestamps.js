globalThis.__nodeTimes ||= Object.create(null);
const __nodeFsTimes = (value) => {
  const path =
    typeof value === "number"
      ? globalThis.__nodeFdPaths[value]
      : nodeFsPath(value);
  if (!path) {
    const error = new Error("ENOENT: no such file or directory");
    error.code = "ENOENT";
    throw error;
  }
  return path;
};
const __nodeFsSetTimes = (value, atime, mtime) => {
  const path = __nodeFsTimes(value);
  globalThis.__nodeTimes[path] = { atime: Number(atime), mtime: Number(mtime) };
};
globalThis.__nodeFs.utimesSync = __nodeFsSetTimes;
globalThis.__nodeFs.lutimesSync = __nodeFsSetTimes;
globalThis.__nodeFs.futimesSync = __nodeFsSetTimes;
const __nodeFsAsyncTimes = (method, value, atime, mtime, callback) => {
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs[method](value, atime, mtime);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.utimes = (value, atime, mtime, callback) =>
  __nodeFsAsyncTimes("utimesSync", value, atime, mtime, callback);
globalThis.__nodeFs.lutimes = (value, atime, mtime, callback) =>
  __nodeFsAsyncTimes("lutimesSync", value, atime, mtime, callback);
globalThis.__nodeFs.futimes = (value, atime, mtime, callback) =>
  __nodeFsAsyncTimes("futimesSync", value, atime, mtime, callback);
