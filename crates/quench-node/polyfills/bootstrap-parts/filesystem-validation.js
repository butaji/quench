const __nodeFsSetMode = (path, mode) => {
  if (mode !== undefined && mode !== null) {
    globalThis.__nodeModes[path] = typeof mode === "string"
      ? parseInt(mode, 8)
      : Number(mode);
  }
};
const __nodeInvalidArgSuffix = (value) => {
  if (value === null || value === undefined) return ` Received ${value}`;
  if (typeof value === "function") return ` Received function ${value.name}`;
  if (typeof value === "object") {
    return ` Received an instance of ${value.constructor?.name || "Object"}`;
  }
  const inspected = typeof value === "string" ? `'${value}'` : String(value);
  return ` Received type ${typeof value} (${inspected})`;
};
const __nodeFsValidateMkdirOptions = (options) => {
  if (
    options &&
    Object.prototype.hasOwnProperty.call(options, "recursive") &&
    typeof options.recursive !== "boolean"
  ) {
    const error = new TypeError(
      `The "options.recursive" property must be of type boolean.${
        __nodeInvalidArgSuffix(
          options.recursive,
        )
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __nodeFsCheckMkdirParents = (path, recursive = false) => {
  const parts = path.split("/").filter(Boolean);
  let prefix = path.startsWith("/") ? "" : ".";
  let firstCreated;
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
    if (!kind && recursive) {
      globalThis.__quench_fs_mkdir(prefix);
      firstCreated ||= prefix;
    }
  }
  return firstCreated;
};
const __nodeFsReadPath = (value, allowFd = true) => {
  if (
    typeof value !== "string" &&
    (typeof value !== "number" || !allowFd) &&
    !(value instanceof NodeBuffer) &&
    !(value instanceof Uint8Array) &&
    !(value instanceof globalThis.__nodeURL)
  ) {
    const error = new TypeError(
      `The "path" argument must be of type string or an instance of Buffer or URL.${
        __nodeInvalidArgSuffix(
          value,
        )
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const path = typeof value === "number"
    ? globalThis.__nodeFdPaths[value]
    : nodePathValue(value);
  if (path) return path;
  const error = new Error("EBADF: bad file descriptor");
  error.code = "EBADF";
  throw error;
};
const __nodeFsPathOnly = (value) => __nodeFsReadPath(value, false);
const __nodeFsCopyPath = (value, name) => {
  try {
    return __nodeFsPathOnly(value);
  } catch (error) {
    if (error.code === "ERR_INVALID_ARG_TYPE") {
      error.message = error.message.replace('"path"', `"${name}"`);
    }
    throw error;
  }
};
const __nodeFsCopyExclusiveError = (destination, source, mode) => {
  if (!(mode & 1)) return;
  let exists = false;
  try {
    exists = Boolean(globalThis.__quench_fs_kind(destination));
  } catch (_) {}
  if (!exists) return;
  const error = new Error(
    `EEXIST: file already exists, copyfile '${source}' -> '${destination}'`,
  );
  error.errno = -17;
  error.code = "EEXIST";
  error.syscall = "copyfile";
  error.path = source;
  error.dest = destination;
  throw error;
};
const __nodeFsApplyMkdirMode = (path, options) => {
  const mode = typeof options === "object" ? options.mode : options;
  if (mode === undefined) {
    globalThis.__nodeModes[path] = 0o777 & ~process.umask();
    return;
  }
  const numericMode = typeof mode === "string"
    ? parseInt(mode, 8)
    : Number(mode);
  globalThis.__nodeModes[path] = numericMode & 0o777;
};
const __nodeFsCreateMkdir = (path, options, targetKind) => {
  const firstCreated = __nodeFsCheckMkdirParents(path, options?.recursive);
  const result = globalThis.__quench_fs_mkdir(path);
  __nodeFsApplyMkdirMode(path, options);
  return (
    firstCreated ||
    (options?.recursive && targetKind !== "directory" ? path : result)
  );
};
const __nodeFsReadBytes = (path, options) => {
  try {
    return globalThis.__quench_fs_read_bytes(path);
  } catch (error) {
    const flag = typeof options === "object" && options
      ? options.flag
      : undefined;
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
  if (!options || typeof options !== "object" || options.buffer === undefined) {
    return undefined;
  }
  const buffer = NodeBuffer.from(bytes);
  const target = typeof options.buffer === "function"
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
  if (ArrayBuffer.isView(data)) {
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  }
  if (
    options?.encoding &&
    options.encoding !== "utf8" &&
    options.encoding !== "utf-8"
  ) {
    return NodeBuffer.from(String(data), options.encoding);
  }
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
      'The "options.flush" property must be of type boolean',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __nodeFsWriteFinalize = (path, options, result) => {
  if (options?.flush) __nodeFsFlush(path);
  if (options?.mode !== undefined) {
    globalThis.__nodeModes[path] = Number(options.mode);
  }
  return result;
};
const __nodeFsReadFileOutput = (bytes, options) => {
  if (options === undefined || options === null) return NodeBuffer.from(bytes);
  const encoding = typeof options === "string"
    ? options
    : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    const error = new TypeError(`Unknown encoding: ${encoding}`);
    error.code = encoding === "test"
      ? "ERR_INVALID_ARG_VALUE"
      : "ERR_UNKNOWN_ENCODING";
    throw error;
  }
  const buffered = __nodeFsReadWithBuffer(bytes, options, encoding);
  if (buffered !== undefined) return buffered;
  return NodeBuffer.from(bytes).toString(encoding || "utf8");
};
globalThis.__nodeFs = {};
Object.assign(globalThis.__nodeFs, {
  _toUnixTimestamp: (value) => {
    const seconds = value instanceof Date
      ? value.getTime() / 1000
      : Number(value);
    return seconds < 0 ? Date.now() / 1000 : seconds;
  },
  constants: new Proxy(
    Object.assign(
      {},
      {
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
        UV_FS_COPYFILE_FICLONE_FORCE: 4,
        UV_FS_SYMLINK_DIR: 1,
        UV_FS_SYMLINK_JUNCTION: 2,
        S_IFMT: 0o170000,
        S_IFREG: 0o100000,
        S_IFDIR: 0o040000,
        S_IFCHR: 0o020000,
        S_IFBLK: 0o060000,
        S_IFIFO: 0o010000,
        S_IFLNK: 0o120000,
        S_IFSOCK: 0o140000,
        S_IRWXU: 0o700,
        S_IRUSR: 0o400,
        S_IWUSR: 0o200,
        S_IXUSR: 0o100,
        S_IRWXG: 0o070,
        S_IRGRP: 0o040,
        S_IWGRP: 0o020,
        S_IXGRP: 0o010,
        S_IRWXO: 0o007,
        S_IROTH: 0o004,
        S_IWOTH: 0o002,
        S_IXOTH: 0o001,
        O_NOCTTY: 256,
        O_DIRECTORY: 65536,
        O_NOATIME: 262144,
        O_NOFOLLOW: 131072,
        O_SYMLINK: 2097152,
        O_DIRECT: 16384,
        O_NONBLOCK: 2048,
        UV_FS_O_FILEMAP: 0,
      },
    ),
    { getPrototypeOf: () => null },
  ),
  existsSync: (value) => globalThis.__quench_fs_exists(nodePathValue(value)),
  exists: globalThis.__nodeFsExists,
  mkdtempSync: (prefix, options) => {
    const encoding = typeof options === "string"
      ? options
      : options && options.encoding;
    if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
      const error = new TypeError(
        `The argument 'encoding' is invalid. Received '${encoding}'`,
      );
      error.code = encoding === "test"
        ? "ERR_INVALID_ARG_VALUE"
        : "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    if (
      typeof prefix !== "string" &&
      !(prefix instanceof Uint8Array) &&
      !(prefix instanceof globalThis.__nodeURL)
    ) {
      const error = new TypeError(
        'The "prefix" argument must be of type string or an instance of Buffer or URL',
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
        'The "options" argument must be a string or an object',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const path = globalThis.__quench_fs_mkdtemp(nodePathValue(prefix));
    globalThis.__nodeModes[path] = 0o700 & ~process.umask();
    return path;
  },
  mkdtempDisposableSync: (prefix, options) => {
    const path = globalThis.__nodeFs.mkdtempSync(prefix, options);
    const removalPath = globalThis.__nodePath.resolve(path);
    let removed = false;
    const remove = () => {
      if (removed) return;
      removed = true;
      try {
        globalThis.__nodeFs.rmdirSync(removalPath);
      } catch (error) {
        removed = false;
        if (String(error?.message || error).includes("EACCES")) {
          const failure = new Error(
            `EACCES: permission denied, rmdir '${removalPath}'`,
          );
          failure.code = "EACCES";
          failure.syscall = "rmdir";
          failure.path = removalPath;
          throw failure;
        }
        throw error;
      }
    };
    Symbol.dispose ||= Symbol("Symbol.dispose");
    return { path, remove, [Symbol.dispose]: remove };
  },
  readFileSync: (value, options) => {
    const path = __nodeFsReadPath(value);
    const encoding = typeof options === "string"
      ? options
      : options && options.encoding;
    if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
      const error = new TypeError(
        `The argument 'encoding' is invalid. Received '${encoding}'`,
      );
      error.code = encoding === "test"
        ? "ERR_INVALID_ARG_VALUE"
        : "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    if (encoding === "utf8" || encoding === "utf-8") {
      try {
        return globalThis.__quench_fs_read_text(path);
      } catch (_) {}
    }
    const bytes = __nodeFsReadBytes(path, options);
    return __nodeFsReadFileOutput(bytes, options);
  },
  writeFileSync: (value, data, options = {}) => {
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
    const path = typeof value === "number"
      ? globalThis.__nodeFdPaths[value]
      : nodeFsPath(value);
    if (!path) {
      const error = new Error("EBADF: bad file descriptor");
      error.code = "EBADF";
      throw error;
    }
    __nodeFsValidateFlush(options);
    const bytes = __nodeFsWriteBytes(data, options);
    if (options?.flag === "a") return __nodeFsAppendBytes(path, bytes);
    const result = globalThis.__quench_fs_write_bytes(path, Array.from(bytes));
    return __nodeFsWriteFinalize(path, options, result);
  },
  openSync: (value, flags = "r", mode) => {
    const path = nodeFsPath(value);
    __nodeFsValidateMode(mode);
    const createsFile = typeof flags === "number" &&
      (flags & globalThis.__nodeFs.constants.O_CREAT) !== 0;
    const flag = typeof flags === "number"
      ? (createsFile ? "w" : "r")
      : String(flags);
    if (
      !createsFile &&
      !/^[wax]/.test(flag) &&
      !globalThis.__quench_fs_access(path)
    ) {
      const error = new Error(
        `ENOENT: no such file or directory, open '${path}'`,
      );
      error.code = "ENOENT";
      error.syscall = "open";
      error.path = path;
      throw error;
    }
    const openDescriptors = Object.keys(globalThis.__nodeFdPaths).map(Number);
    const fd = Math.max(-1, ...openDescriptors) + 1 ||
      globalThis.__quench_fs_open(path, flag);
    globalThis.__quench_fs_open(path, flag);
    globalThis.__nodeFdPaths[fd] = path;
    globalThis.__nodeFdPositions[fd] = 0;
    __nodeFsSetMode(path, mode);
    return fd;
  },
  closeSync: (fd) => {
    if (typeof fd !== "number") {
      const error = new TypeError(
        `The "fd" argument must be of type number.${
          globalThis.__nodeCommon.invalidArgTypeHelper(
            fd,
          )
        }`,
      );
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
    const times = globalThis.__nodeTimes?.[path];
    const date = times ? new Date(times.mtime) : new Date();
    const stats = new globalThis.__nodeStats(file, kind === "directory", date);
    if (times) {
      stats.atime = new Date(times.atime);
      stats.atimeMs = times.atime;
      stats.mtime = new Date(times.mtime);
      stats.mtimeMs = times.mtime;
    }
    if (file) stats.size = globalThis.__quench_fs_read_bytes(path).length;
    stats.mode = globalThis.__nodeModes[path] ||
      (file ? 0o666 & ~process.umask() : 0);
    return stats;
  },
});
