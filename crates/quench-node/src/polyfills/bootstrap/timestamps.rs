//! Polyfill: `timestamps`

pub const JS: &str = quench_js_check::checked_js!(r#"globalThis.__nodeTimes ||= Object.create(null);
const __nodeFsTimes = (value) => __nodeFsPathOnly(value);
const __nodeFsSetTimes = (value, atime, mtime) => {
  const path = __nodeFsTimes(value);
  try {
    globalThis.__quench_fs_kind(path);
  } catch (_) {
    const error = new Error("ENOENT: no such file or directory");
    error.code = "ENOENT";
    throw error;
  }
  globalThis.__nodeTimes[path] = { atime: Number(atime), mtime: Number(mtime) };
};
const __nodeFsSetLinkTimes = (value, atime, mtime) => {
  const path = __nodeFsPathOnly(value);
  globalThis.__nodeTimes[path] = { atime: Number(atime), mtime: Number(mtime) };
};
const __nodeFsSetFdTimes = (fd, atime, mtime) => {
  if (typeof fd !== "number") {
    throw Object.assign(new TypeError('The "fd" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!Number.isInteger(fd) || fd < 0) {
    throw Object.assign(new RangeError('The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received -1'), { code: "ERR_OUT_OF_RANGE" });
  }
  __nodeFsSetTimes(globalThis.__nodeFdPaths[fd], atime, mtime);
};
globalThis.__nodeFs.utimesSync = __nodeFsSetTimes;
globalThis.__nodeFs.lutimesSync = __nodeFsSetLinkTimes;
globalThis.__nodeFs.futimesSync = __nodeFsSetFdTimes;
const __nodeFsAsyncTimes = (method, value, atime, mtime, callback) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  try {
    globalThis.__nodeFs[method](value, atime, mtime);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
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
"#);
