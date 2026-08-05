const __nodeFsValidateMode = (mode) => {
  if (
    mode !== undefined &&
    mode !== null &&
    typeof mode !== "number" &&
    typeof mode !== "string"
  ) {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __nodeFsSetMode = (path, mode) => {
  if (mode !== undefined && mode !== null)
    globalThis.__nodeModes[path] =
      typeof mode === "string" ? parseInt(mode, 8) : Number(mode);
};
const __nodeFsValidateMkdirOptions = (options) => {
  if (
    options &&
    Object.prototype.hasOwnProperty.call(options, "recursive") &&
    typeof options.recursive !== "boolean"
  ) {
    const error = new TypeError(
      'The "options.recursive" property must be of type boolean.'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __nodeFsCheckMkdirParents = (path) => {
  const parts = path.split("/").filter(Boolean);
  let prefix = path.startsWith("/") ? "" : ".";
  for (const part of parts.slice(0, -1)) {
    prefix += `/${part}`;
    let kind;
    try {
      kind = globalThis.__quench_fs_kind(prefix);
    } catch (_) {
      kind = undefined;
    }
    if (kind === "file") {
      const error = new Error(`ENOTDIR: not a directory, mkdir '${path}'`);
      error.code = "ENOTDIR";
      error.syscall = "mkdir";
      error.path = path;
      throw error;
    }
  }
};
const __nodeFsReadPath = (value) => {
  if (
    value === null ||
    (typeof value === "object" &&
      !(value instanceof NodeBuffer) &&
      !(value instanceof Uint8Array) &&
      !(value instanceof globalThis.__nodeURL))
  ) {
    const error = new TypeError(
      'The "path" argument must be of type string or an instance of Buffer or URL'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const path =
    typeof value === "number"
      ? globalThis.__nodeFdPaths[value]
      : nodePathValue(value);
  if (path) return path;
  const error = new Error("EBADF: bad file descriptor");
  error.code = "EBADF";
  throw error;
};
const __nodeFsReadBytes = (path, options) => {
  try {
    return globalThis.__quench_fs_read_bytes(path);
  } catch (error) {
    const flag =
      typeof options === "object" && options ? options.flag : undefined;
    if (flag === "a" || flag === "a+") {
      globalThis.__quench_fs_write_bytes(path, []);
      globalThis.__nodeModes[path] = 0o666 & ~process.umask();
      return [];
    }
    if (!error.code) {
      error.code = "ENOENT";
      error.syscall = "open";
      error.path = path;
    }
    throw error;
  }
};
const __nodeFsReadWithBuffer = (bytes, options, encoding) => {
  if (!options || typeof options !== "object" || options.buffer === undefined)
    return undefined;
  const buffer = NodeBuffer.from(bytes);
  const target =
    typeof options.buffer === "function"
      ? options.buffer(buffer.length)
      : options.buffer;
  if (!(target instanceof Uint8Array)) {
    const error = new TypeError('The "buffer" option must return a Buffer');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  target.set(buffer.subarray(0, target.length));
  return encoding
    ? target.toString(encoding)
    : target.subarray(0, buffer.length);
};
const __nodeFsWriteBytes = (data, options) => {
  if (data instanceof Uint8Array) return data;
  if (ArrayBuffer.isView(data))
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  if (
    options?.encoding &&
    options.encoding !== "utf8" &&
    options.encoding !== "utf-8"
  )
    return NodeBuffer.from(String(data), options.encoding);
  return NodeBuffer.from(String(data));
};
const __nodeFsAppendBytes = (path, bytes) => {
  let existing = [];
  try {
    existing = globalThis.__quench_fs_read_bytes(path);
  } catch (_) {}
  return globalThis.__quench_fs_write_bytes(path, [...existing, ...bytes]);
};
const __nodeFsFlush = (path) => {
  const fd = globalThis.__nodeFs.openSync(path, "r");
  globalThis.__nodeFs.fsyncSync(fd);
  globalThis.__nodeFs.closeSync(fd);
};
const __nodeFsValidateFlush = (options) => {
  if (options?.flush !== undefined && typeof options.flush !== "boolean") {
    const error = new TypeError(
      'The "options.flush" property must be of type boolean'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __nodeFsWriteFinalize = (path, options, result) => {
  if (options?.flush) __nodeFsFlush(path);
  if (options?.mode !== undefined)
    globalThis.__nodeModes[path] = Number(options.mode);
  return result;
};
globalThis.__nodeFs = {};
Object.assign(globalThis.__nodeFs, {
  constants: {
    F_OK: 0,
    R_OK: 4,
    W_OK: 2,
    X_OK: 1,
    O_APPEND: 1024,
    O_CREAT: 64,
    O_EXCL: 128,
    O_RDONLY: 0,
    O_RDWR: 2,
    O_SYNC: 1052672,
    O_DSYNC: 4194304,
    O_TRUNC: 512,
    O_WRONLY: 1,
    UV_DIRENT_UNKNOWN: 0,
    UV_DIRENT_FILE: 1,
    UV_DIRENT_DIR: 2,
    UV_DIRENT_LINK: 3,
    UV_DIRENT_FIFO: 4,
    UV_DIRENT_SOCKET: 5,
    UV_DIRENT_CHAR: 6,
    UV_DIRENT_BLOCK: 7,
    COPYFILE_EXCL: 1,
    COPYFILE_FICLONE: 2,
    COPYFILE_FICLONE_FORCE: 4,
    UV_FS_COPYFILE_EXCL: 1,
    UV_FS_COPYFILE_FICLONE: 2,
    UV_FS_COPYFILE_FICLONE_FORCE: 4
  },
  existsSync: (value) => globalThis.__quench_fs_exists(nodePathValue(value)),
  mkdtempSync: (prefix, options) => {
    if (
      typeof prefix !== "string" &&
      !(prefix instanceof Uint8Array) &&
      !(prefix instanceof globalThis.__nodeURL)
    ) {
      const error = new TypeError(
        'The "prefix" argument must be of type string or an instance of Buffer or URL'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      options !== undefined &&
      typeof options !== "string" &&
      (typeof options !== "object" || options === null)
    ) {
      const error = new TypeError(
        'The "options" argument must be a string or an object'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return globalThis.__quench_fs_mkdtemp(nodePathValue(prefix));
  },
  readFileSync: (value, options) => {
    const path = __nodeFsReadPath(value);
    const bytes = __nodeFsReadBytes(path, options);
    const hex = NodeBuffer.from(bytes).toString("hex");
    if (options === undefined || options === null)
      return NodeBuffer.from(bytes);
    const encoding =
      typeof options === "string" ? options : options && options.encoding;
    if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
      const error = new TypeError(`Unknown encoding: ${encoding}`);
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    const buffered = __nodeFsReadWithBuffer(bytes, options, encoding);
    if (buffered !== undefined) return buffered;
    if (encoding === "hex" || encoding === "base64")
      return NodeBuffer.from(hex, "hex").toString(encoding);
    return globalThis.__quench_fs_read_file(path);
  },
  writeFileSync: (value, data, options = {}) => {
    const path = __nodeFsReadPath(value);
    __nodeFsValidateFlush(options);
    const bytes = __nodeFsWriteBytes(data, options);
    if (options?.flag === "a") return __nodeFsAppendBytes(path, bytes);
    const result = globalThis.__quench_fs_write_bytes(path, Array.from(bytes));
    return __nodeFsWriteFinalize(path, options, result);
  },
  openSync: (value, flags = "r", mode) => {
    const path = nodeFsPath(value);
    __nodeFsValidateMode(mode);
    const flag = String(flags);
    if (!/^[wax]/.test(flag) && !globalThis.__quench_fs_access(path)) {
      const error = new Error(
        `ENOENT: no such file or directory, open '${path}'`
      );
      error.code = "ENOENT";
      error.syscall = "open";
      error.path = path;
      throw error;
    }
    const fd = globalThis.__quench_fs_open(path, flag);
    globalThis.__nodeFdPaths[fd] = path;
    globalThis.__nodeFdPositions[fd] = 0;
    __nodeFsSetMode(path, mode);
    return fd;
  },
  closeSync: (fd) => {
    if (typeof fd !== "number") {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (globalThis.__nodeFdPaths[fd] === undefined) {
      const error = new Error("EBADF: bad file descriptor");
      error.code = "EBADF";
      error.syscall = "close";
      throw error;
    }
    delete globalThis.__nodeFdPaths[fd];
    delete globalThis.__nodeFdPositions[fd];
  },
  statSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    let kind;
    try {
      kind = globalThis.__quench_fs_kind(path);
    } catch (error) {
      if (options && options.throwIfNoEntry === false) return undefined;
      if (!error.code) {
        error.code = "ENOENT";
        error.syscall = "stat";
        error.path = path;
        error.message = `ENOENT: no such file or directory, stat '${path}'`;
      }
      throw error;
    }
    const file = kind === "file";
    const date = new Date();
    const stats = new globalThis.__nodeStats(file, kind === "directory", date);
    if (file) stats.size = globalThis.__quench_fs_read_bytes(path).length;
    stats.mode =
      globalThis.__nodeModes[path] || (file ? 0o666 & ~process.umask() : 0);
    return stats;
  },
  mkdirSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    __nodeFsValidateMkdirOptions(options);
    let targetKind;
    try {
      targetKind = globalThis.__quench_fs_kind(path);
    } catch (_) {
      targetKind = undefined;
    }
    if (targetKind === "file") {
      const error = new Error(`EEXIST: file already exists, mkdir '${path}'`);
      error.code = "EEXIST";
      error.syscall = "mkdir";
      error.path = path;
      throw error;
    }
    if (targetKind === "directory" && !(options && options.recursive)) {
      const error = new Error(`EEXIST: file already exists, mkdir '${path}'`);
      error.code = "EEXIST";
      error.syscall = "mkdir";
      error.path = path;
      throw error;
    }
    __nodeFsCheckMkdirParents(path);
    try {
      return globalThis.__quench_fs_mkdir(path);
    } catch (_) {
      const error = new Error(
        `ENOENT: no such file or directory, mkdir '${path}'`
      );
      error.code = "ENOENT";
      error.syscall = "mkdir";
      error.path = path;
      throw error;
    }
  },
  readdirSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    let kind;
    try {
      kind = globalThis.__quench_fs_kind(path);
    } catch (_) {
      kind = undefined;
    }
    if (kind === "file") {
      const error = new Error(`ENOTDIR: not a directory, scandir '${path}'`);
      error.code = "ENOTDIR";
      error.syscall = "scandir";
      error.path = path;
      throw error;
    }
    const entries = globalThis.__quench_fs_readdir(path).sort();
    if (!options || !options.withFileTypes) return entries;
    return entries.map((name) => {
      const dirent = new globalThis.__nodeFs.Dirent(
        name,
        (() => {
          try {
            return (
              globalThis.__quench_fs_kind(`${path}/${name}`) === "directory"
            );
          } catch (_) {
            return false;
          }
        })()
      );
      dirent.parentPath = path;
      return dirent;
    });
  },
  rmdirSync: (value) => globalThis.__quench_fs_remove_dir(String(value)),
  renameSync: (from, to) =>
    globalThis.__quench_fs_rename(nodeFsPath(from), nodeFsPath(to)),
  unlinkSync: (value) => globalThis.__quench_fs_unlink(String(value)),
  truncateSync: (value, length = 0) => {
    if (typeof length !== "number" || !Number.isFinite(length)) {
      const error = new TypeError('The "len" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(length)) {
      const error = new RangeError('The value of "len" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const path =
      typeof value === "number"
        ? globalThis.__nodeFdPaths[value]
        : nodeFsPath(value);
    if (!path) throw new Error("EBADF");
    return globalThis.__quench_fs_truncate(path, Math.max(0, Number(length)));
  }
});
