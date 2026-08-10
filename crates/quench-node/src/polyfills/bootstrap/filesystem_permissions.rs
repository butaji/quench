//! Polyfill: `filesystem-permissions`

pub const JS: &str = r#"const nodeMode = (mode) => {
  if (typeof mode !== "number" && typeof mode !== "string") {
    throw Object.assign(new TypeError('The "mode" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  const value = typeof mode === "string" ? parseInt(mode, 8) : Number(mode);
  if (typeof mode === "string" && !/^0?[0-7]+$/.test(mode)) {
    throw Object.assign(new TypeError(`The "mode" argument is invalid: ${mode}`), { code: "ERR_INVALID_ARG_VALUE" });
  }
  if (!Number.isFinite(value) || !Number.isInteger(value)) {
    throw Object.assign(new RangeError(`The value of "mode" is out of range. It must be an integer. Received ${mode}`), { code: "ERR_OUT_OF_RANGE" });
  }
  if (value < 0 || value > 0xffffffff) {
    throw Object.assign(new RangeError(`The value of "mode" is out of range. It must be >= 0 && <= 4294967295. Received ${mode}`), { code: "ERR_OUT_OF_RANGE" });
  }
  return value;
};
const nodeFd = (fd) => {
  if (typeof fd !== "number") {
    const error = new TypeError(
      `The "fd" argument must be of type number.${__nodeInvalidArgSuffix(fd)}`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(fd)) {
    throw Object.assign(new RangeError(`The value of "fd" is out of range. It must be an integer. Received ${fd}`), { code: "ERR_OUT_OF_RANGE" });
  }
  if (fd < 0 || fd > 0x7fffffff) {
    throw Object.assign(new RangeError(`The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received ${fd}`), { code: "ERR_OUT_OF_RANGE" });
  }
};
globalThis.__nodeFs.fchmodSync = (fd, mode) => {
  nodeFd(fd);
  const value = nodeMode(mode);
  if (globalThis.__nodeFdPaths[fd]) {
    globalThis.__nodeFs.chmodSync(globalThis.__nodeFdPaths[fd], value);
  }
};
globalThis.__nodeFs.fchmod = (fd, mode, callback) => {
  nodeFd(fd);
  nodeMode(mode);
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  globalThis.__nodeFs.fchmodSync(fd, mode);
  try {
    callback(null);
  } catch (error) {
    callback(error);
  }
};
globalThis.__nodeFs.lchmodSync = (value, mode) => {
  const path = __nodeFsPathOnly(value);
  const valueMode = nodeMode(mode);
  globalThis.__nodeModes[path] = valueMode;
};
globalThis.__nodeFs.lchmod = (value, mode, callback) => {
  if (typeof mode === "function") {
    callback = mode;
    mode = 0o666;
  }
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  globalThis.__nodeFs.lchmodSync(value, mode);
  queueMicrotask(() => callback(null));
};
const nodeOwner = (owner, name) => {
  if (typeof owner !== "number") {
    throw Object.assign(new TypeError(`The "${name}" argument must be of type number`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!Number.isFinite(owner) || !Number.isInteger(owner)) {
    throw Object.assign(new RangeError(`The value of "${name}" is out of range. It must be an integer. Received ${owner}`), { code: "ERR_OUT_OF_RANGE" });
  }
  if (owner < -1 || owner > 0xffffffff) {
    throw Object.assign(new RangeError(`The value of "${name}" is out of range. It must be >= -1 && <= 4294967295. Received ${owner}`), { code: "ERR_OUT_OF_RANGE" });
  }
};
globalThis.__nodeFs.lchownSync = (value, uid, gid) => {
  nodeOwner(uid, "uid");
  nodeOwner(gid, "gid");
  __nodeFsPathOnly(value);
};
globalThis.__nodeFs.lchown = (value, uid, gid, callback) => {
  nodeOwner(uid, "uid");
  nodeOwner(gid, "gid");
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  globalThis.__nodeFs.lchownSync(value, uid, gid);
  queueMicrotask(() => callback(null));
};
globalThis.__nodeFs.chownSync = (value, uid, gid) =>
  globalThis.__nodeFs.lchownSync(value, uid, gid);
globalThis.__nodeFs.chown = (value, uid, gid, callback) =>
  globalThis.__nodeFs.lchown(value, uid, gid, callback);
globalThis.__nodeFs.fchownSync = (fd, uid, gid) => {
  nodeFd(fd);
  nodeOwner(uid, "uid");
  nodeOwner(gid, "gid");
};
globalThis.__nodeFs.fchown = (fd, uid, gid, callback) => {
  nodeFd(fd);
  nodeOwner(uid, "uid");
  nodeOwner(gid, "gid");
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  queueMicrotask(() => callback(null));
};
"#;
