globalThis.__nodeFs.writeFile = (value, data, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const encoding = typeof options === "string"
    ? options
    : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    const error = new TypeError(
      `The argument 'encoding' is invalid. Received '${encoding}'`,
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  if (options && options.signal) {
    queueMicrotask(() => {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      callback(error);
    });
    return;
  }
  if (options && options.flag === "r") {
    queueMicrotask(() => {
      const error = new Error("EBADF: bad file descriptor, write");
      error.code = "EBADF";
      callback(error);
    });
    return;
  }
  queueMicrotask(() => {
    try {
      if (typeof value === "number") {
        globalThis.__nodeFs.writeSync(value, data);
      } else {
        const path = nodeFsPath(value);
        const bytes = __nodeFsWriteBytes(data, options || {});
        globalThis.__quench_fs_write_bytes(path, Array.from(bytes));
      }
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
const __nodeFsPromisedReadOptions = (buffer, offset, length, position) => {
  if (!offset || typeof offset !== "object") {
    return { target: buffer, start: offset, size: length, at: position };
  }
  const options = offset;
  const target = options.buffer || NodeBuffer.alloc(16384);
  const start = options.offset == null ? 0 : options.offset;
  const size = options.length === undefined
    ? target.length - start
    : options.length;
  const at = options.position;
  return { target, start, size, at };
};
const __nodeFsPromisedRead = (fd, buffer, offset, length, position) => {
  const { target, start, size, at } = __nodeFsPromisedReadOptions(
    buffer,
    offset,
    length,
    position,
  );
  if (target.length === 0 && Number(size) > 0) {
    const error = new TypeError("The buffer is empty");
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  const bytesRead = globalThis.__nodeFs.readSync(
    fd,
    target,
    start || 0,
    size === undefined ? target.length : size,
    at === undefined ? null : at,
  );
  return { bytesRead, buffer: target };
};
globalThis.__nodeFs.promises = {
  open: (value, flags = "r", mode) =>
    new Promise((resolve, reject) => {
      const onOpen = (error, fd) =>
        error ? reject(error) : resolve({
          fd,
          close: () => Promise.resolve(),
          read: (buffer, offset, length, position) =>
            Promise.resolve().then(() =>
              __nodeFsPromisedRead(fd, buffer, offset, length, position)
            ),
        });
      if (mode === undefined) globalThis.__nodeFs.open(value, flags, onOpen);
      else globalThis.__nodeFs.open(value, flags, mode, onOpen);
    }),
  readFile: (value, options) =>
    value && typeof value === "object" && typeof value.fd === "number"
      ? value.readFile(options)
      : new Promise((resolve, reject) =>
        globalThis.__nodeFs.readFile(
          value,
          options,
          (error, data) => error ? reject(error) : resolve(data),
        )
      ),
  writeFile: (value, data, options) =>
    new Promise((resolve, reject) =>
      queueMicrotask(() =>
        globalThis.__nodeFs.writeFile(
          value,
          data,
          options,
          (error) => error ? reject(error) : resolve(),
        )
      )
    ),
  appendFile: (value, data, options) =>
    new Promise((resolve, reject) => {
      const target =
        value && typeof value === "object" && typeof value.fd === "number"
          ? value.fd
          : value;
      globalThis.__nodeFs.appendFile(target, data, options, (error) =>
        error ? reject(error) : resolve());
    }),
  access: async (value, mode = 0) => {
    globalThis.__nodeFs.accessSync(value, mode);
  },
  truncate: (value, length = 0) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.truncateSync(value, length)
    ),
  ftruncate: (fd, length = 0) =>
    Promise.resolve().then(() => globalThis.__nodeFs.ftruncateSync(fd, length)),
  fsync: (fd) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fsyncSync(fd)),
  fdatasync: (fd) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fdatasyncSync(fd)),
  rm: (value, options) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.rm(
        value,
        options,
        (error) => error ? reject(error) : resolve(),
      )
    ),
  opendir: (value, options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.opendirSync(value, options)
    ),
  symlink: (target, link, type) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.symlink(
        target,
        link,
        type,
        (error) => error ? reject(error) : resolve(),
      )
    ),
  readlink: (value, options) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.readlink(
        value,
        options,
        (error, result) => error ? reject(error) : resolve(result),
      )
    ),
  realpath: (value, options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.realpathSync(value, options)
    ),
  fstat: (fd, options) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fstatSync(fd, options)),
  statfs: (value, options) =>
    Promise.resolve().then(() => {
      if (value === undefined || value === null) {
        const error = new TypeError(
          'The "path" argument must be of type string or an instance of Buffer or URL',
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      return globalThis.__nodeFs.statfsSync(value, options);
    }),
  fchmod: (fd, mode) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fchmodSync(fd, mode)),
  chmod: (value, mode) =>
    Promise.resolve().then(() => globalThis.__nodeFs.chmodSync(value, mode)),
  rename: (from, to) =>
    Promise.resolve().then(() =>
      globalThis.__quench_fs_rename(nodeFsPath(from), nodeFsPath(to))
    ),
  unlink: (value) =>
    Promise.resolve().then(() =>
      globalThis.__quench_fs_unlink(nodeFsPath(value))
    ),
  copyFile: (from, to, mode = 0) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.copyFileSync(from, to, mode)
    ),
  rmdir: (value, options) =>
    Promise.resolve().then(() => globalThis.__nodeFs.rmdirSync(value, options)),
  mkdtemp: (prefix) =>
    Promise.resolve().then(() => globalThis.__nodeFs.mkdtempSync(prefix)),
  mkdtempDisposable: (prefix, options) =>
    Promise.resolve().then(() => {
      const path = globalThis.__nodeFs.mkdtempSync(prefix, options);
      const removalPath = globalThis.__nodePath.resolve(path);
      let removed = false;
      const remove = async () => {
        if (removed) return;
        removed = true;
        try {
          globalThis.__nodeFs.rmdirSync(removalPath);
        } catch (error) {
          removed = false;
          throw error;
        }
      };
      Symbol.asyncDispose ||= Symbol("Symbol.asyncDispose");
      return { path, remove, [Symbol.asyncDispose]: remove };
    }),
  readv: (fd, buffers, position) =>
    Promise.resolve().then(() => {
      const bytesRead = globalThis.__nodeFs.readvSync(fd, buffers, position);
      return { bytesRead, buffers };
    }),
  writev: (fd, buffers, position) =>
    Promise.resolve().then(() => {
      const bytesWritten = globalThis.__nodeFs.writevSync(
        fd,
        buffers,
        position,
      );
      return { bytesWritten, buffers };
    }),
  mkdir: (value) =>
    Promise.resolve().then(() => globalThis.__nodeFs.mkdirSync(value)),
  readdir: (value, options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.readdirSync(value, options)
    ),
  stat: (value, options) =>
    Promise.resolve().then(() => globalThis.__nodeFs.statSync(value, options)),
  lstat: (value, options) =>
    Promise.resolve().then(() => globalThis.__nodeFs.lstatSync(value, options)),
  link: (existing, link) =>
    Promise.resolve().then(() => globalThis.__nodeFs.linkSync(existing, link)),
};
const __nodePromiseOpen = globalThis.__nodeFs.promises.open;
