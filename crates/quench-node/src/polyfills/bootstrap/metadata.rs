//! Polyfill: `metadata`

pub const JS: &str = r#"globalThis.__nodeFs.truncate = (value, length = 0, callback) => {
  if (typeof length === "function") {
    callback = length;
    length = 0;
  }
  __validateTruncateLength(length);
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  const path = typeof value === "number"
    ? globalThis.__nodeFdPaths[value]
    : nodeFsPath(value);
  if (__truncateMissingPath(path, callback)) return;
  queueMicrotask(() => {
    try {
      globalThis.__quench_fs_truncate(path, Math.max(0, Number(length)));
    } catch (error) {
      if (
        error.code === "ENOENT" ||
        String(error.message).includes("no such file")
      ) {
        const missing = new Error(
          `ENOENT: no such file or directory, open '${path}'`,
        );
        missing.code = "ENOENT";
        missing.path = path;
        missing.syscall = "open";
        callback(missing);
        return;
      }
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.ftruncate = (fd, length = 0, callback) => {
  if (typeof length === "function") {
    callback = length;
    length = 0;
  }
  __validateTruncateLength(length);
  if (typeof fd !== "number") {
    const error = new TypeError(
      `The "fd" argument must be of type number.${__nodeInvalidArgSuffix(fd)}`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  queueMicrotask(() => {
    try {
      if (length < 0) {
        globalThis.__quench_fs_truncate(globalThis.__nodeFdPaths[fd], 0);
      } else globalThis.__nodeFs.ftruncateSync(fd, length);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.access = (value, mode, callback) => {
  if (typeof mode === "function") {
    callback = mode;
    mode = 0;
  }
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  __nodeFsValidateAccessMode(mode);
  if (typeof value === "number") {
    throw Object.assign(new TypeError('The "path" argument must be of type string or an instance of Buffer or URL'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.accessSync(path, mode);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.fsync = (fd, callback) => {
  if (typeof fd !== "number") {
    throw Object.assign(new TypeError(`The "fd" argument must be of type number. Received ${fd}`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.fsyncSync(fd);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.fdatasync = (fd, callback) => {
  if (typeof fd !== "number") {
    throw Object.assign(new TypeError('The "fd" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.fdatasyncSync(fd);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
const __nodeFsAsyncReadOptions = (
  buffer,
  offset,
  length,
  position,
  callback,
) => {
  if (__nodeFsAsyncReadUsesDefaultBuffer(buffer)) {
    return __nodeFsAsyncReadDefault(buffer);
  }
  if (buffer === null) return __nodeFsAsyncReadDefault(offset);
  if (typeof buffer === "object" && !ArrayBuffer.isView(buffer)) {
    return __nodeFsAsyncReadBufferOptions(buffer, offset);
  }
  if (typeof offset === "function") {
    return {
      buffer,
      offset: 0,
      length: buffer.length,
      position: null,
      callback: offset,
    };
  }
  if (typeof offset === "object" || offset === null || offset === undefined) {
    return __nodeFsAsyncReadOffsetOptions(buffer, offset, length);
  }
  if (typeof position === "function") position = null;
  return { buffer, offset, length, position, callback };
};
const __nodeFsAsyncReadUsesDefaultBuffer = (buffer) =>
  typeof buffer === "function" || buffer === undefined;
const __nodeFsAsyncReadDefault = (callback) => {
  const buffer = NodeBuffer.alloc(16384);
  return { buffer, offset: 0, length: buffer.length, position: null, callback };
};
const __nodeFsAsyncReadBufferOptions = (buffer, callback) => {
  const options = buffer;
  const target = options.buffer === undefined
    ? NodeBuffer.alloc(
      options.length === undefined ? 16384 : Number(options.length),
    )
    : options.buffer;
  const offset = options.offset == null ? 0 : Number(options.offset);
  const length = options.length === undefined
    ? target === null ? 0 : target.length - offset
    : Number(options.length);
  const position = options.position === undefined ? null : options.position;
  return { buffer: target, offset, length, position, callback };
};
const __nodeFsAsyncReadOffsetOptions = (buffer, offset, callback) => {
  const options = offset || {};
  const start = Number(options.offset || 0);
  const length = options.length === undefined
    ? buffer.length - start
    : Number(options.length);
  const position = options.position === undefined ? null : options.position;
  return { buffer, offset: start, length, position, callback };
};
const __nodeFsValidateAsyncReadBuffer = (buffer, length) => {
  if (!(buffer instanceof Uint8Array)) {
    throw Object.assign(new TypeError('The "buffer" argument must be an instance of Buffer, TypedArray, or DataView'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (buffer.length === 0 && Number(length) > 0) {
    throw Object.assign(new TypeError("The argument 'buffer' is empty and cannot be written."), { code: "ERR_INVALID_ARG_VALUE" });
  }
};
const __nodeFsValidateAsyncReadRange = (offset, length) => {
  if (
    !Number.isInteger(offset) ||
    offset < 0 ||
    !Number.isInteger(length) ||
    length < 0
  ) {
    throw Object.assign(new RangeError("The read range is out of range"), { code: "ERR_OUT_OF_RANGE" });
  }
};
const __nodeFsValidateAsyncReadPosition = (position) => {
  if (
    position !== null &&
    position !== undefined &&
    typeof position !== "number" &&
    typeof position !== "bigint"
  ) {
    throw Object.assign(new TypeError('The "position" argument must be of type number or bigint'), { code: "ERR_INVALID_ARG_TYPE" });
  }
};
const __nodeFsValidateAsyncRead = (
  fd,
  buffer,
  offset,
  length,
  position,
  callback,
) => {
  if (typeof fd !== "number") {
    throw Object.assign(new TypeError(`The "fd" argument must be of type number. Received ${fd}`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  __nodeFsValidateAsyncReadBuffer(buffer, length);
  __nodeFsValidateAsyncReadRange(offset, length);
  __nodeFsValidateAsyncReadPosition(position);
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
};
globalThis.__nodeFs.read = (fd, buffer, offset, length, position, callback) => {
  ({ buffer, offset, length, position, callback } = __nodeFsAsyncReadOptions(
    buffer,
    offset,
    length,
    position,
    callback,
  ));
  __nodeFsValidateAsyncRead(fd, buffer, offset, length, position, callback);
  queueMicrotask(() => {
    try {
      const count = globalThis.__nodeFs.readSync(
        fd,
        buffer,
        offset,
        length,
        position,
      );
      callback(null, count, buffer);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.readv = (fd, buffers, position, callback) => {
  if (typeof position === "function") {
    callback = position;
    position = null;
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  if (
    !Array.isArray(buffers) ||
    buffers.some((buffer) => !(buffer instanceof Uint8Array))
  ) {
    throw Object.assign(new TypeError('The "buffers" argument must be an array of Buffer or Uint8Array'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  queueMicrotask(() => {
    try {
      callback(
        null,
        globalThis.__nodeFs.readvSync(fd, buffers, position),
        buffers,
      );
    } catch (error) {
      callback(error);
    }
  });
};
const __nodeFsWriteObjectOptions = (options, callback) => ({
  buffer: options.buffer,
  offset: options.offset || 0,
  length: options.length === undefined
    ? options.buffer && options.buffer.length - (options.offset || 0)
    : options.length,
  position: options.position,
  callback,
});
const __nodeFsWriteOptions = (buffer, offset, length, position, callback) => {
  if (typeof offset === "function") {
    callback = offset;
    offset = undefined;
  }
  if (
    buffer &&
    typeof buffer === "object" &&
    !ArrayBuffer.isView(buffer) &&
    "buffer" in buffer
  ) {
    return __nodeFsWriteObjectOptions(buffer, callback);
  }
  return {
    buffer,
    offset,
    length,
    position: typeof position === "function" ? null : position,
    callback: typeof position === "function" ? position : callback,
  };
};
globalThis.__nodeFs.write = (
  fd,
  buffer,
  offset,
  length,
  position,
  callback,
) => {
  ({ buffer, offset, length, position, callback } = __nodeFsWriteOptions(
    buffer,
    offset,
    length,
    position,
    callback,
  ));
  __nodeFsValidateWrite(fd, buffer, callback);
  queueMicrotask(() => {
    try {
      callback(
        null,
        globalThis.__nodeFs.writeSync(fd, buffer, offset, length, position),
        buffer,
      );
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.writev = (fd, buffers, position, callback) => {
  if (typeof buffers === "function") {
    callback = buffers;
    buffers = undefined;
  }
  if (typeof position === "function") {
    callback = position;
    position = null;
  }
  __nodeFsValidateWritev(fd, buffers, callback);
  queueMicrotask(() => {
    try {
      callback(
        null,
        globalThis.__nodeFs.writevSync(fd, buffers, position),
        buffers,
      );
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeModes = {};
globalThis.__nodeFdPaths = {};
globalThis.__nodeFdPositions = {};
globalThis.__nodeFs.statfsSync = (value, options = {}) => {
  const path = nodeFsPath(value);
  if (!globalThis.__quench_fs_access(path)) throw new Error("ENOENT");
  const values = {
    type: 0,
    bsize: 4096,
    frsize: 4096,
    blocks: 1,
    bfree: 1,
    bavail: 1,
    files: 1,
    ffree: 1,
  };
  if (options && options.bigint) {
    Object.keys(values).forEach((key) => {
      values[key] = BigInt(values[key]);
    });
  }
  return values;
};
globalThis.__nodeFs.statfs = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.statfsSync(path, options));
    } catch (error) {
      callback(error);
    }
  });
};
const __nodeFsValidateSymlink = (target, link, type, callback) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  if (
    (typeof target !== "string" && !(target instanceof Uint8Array)) ||
    (typeof link !== "string" && !(link instanceof Uint8Array))
  ) {
    throw Object.assign(new TypeError('The "target" and "path" arguments must be strings or Buffer'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (
    type !== undefined &&
    type !== "file" &&
    type !== "dir" &&
    type !== "junction"
  ) {
    throw Object.assign(new TypeError('The "type" argument is invalid'), { code: "ERR_INVALID_ARG_VALUE" });
  }
};
globalThis.__nodeFs.symlink = (target, link, type, callback) => {
  if (typeof type === "function") {
    callback = type;
    type = undefined;
  }
  __nodeFsValidateSymlink(target, link, type, callback);
  const source = nodePathValue(target);
  const destination = nodeFsPath(link);
  queueMicrotask(() => {
    try {
      globalThis.__quench_fs_symlink(source, destination);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
"#;
