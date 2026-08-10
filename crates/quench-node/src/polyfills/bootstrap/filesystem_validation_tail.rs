//! Polyfill: `filesystem-validation-tail`

pub const JS: &str = r#"const __nodeFsStatsBigInt = (stats, options) => {
  if (options?.bigint !== true) return stats;
  for (const name of "dev mode nlink uid gid rdev blksize ino size blocks atimeMs mtimeMs ctimeMs birthtimeMs".split(
    " "
  ))
    stats[name] = BigInt(Math.trunc(Number(stats[name]) || 0));
  for (const name of ["atime", "mtime", "ctime", "birthtime"]) {
    stats[`${name}Ns`] = BigInt(
      Math.trunc(Number(stats[`${name}Ms`]) * 1_000_000)
    );
  }
  return stats;
};
Object.assign(globalThis.__nodeFs, {
  fstatSync: (fd) => {
    if (typeof fd !== "number" || globalThis.__nodeFdPaths[fd] === undefined) {
      const error = new Error("EBADF: bad file descriptor, fstat");
      error.code = "EBADF";
      error.syscall = "fstat";
      throw error;
    }
    return globalThis.__nodeFs.statSync(globalThis.__nodeFdPaths[fd]);
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
    try {
      return __nodeFsCreateMkdir(path, options, targetKind);
    } catch (error) {
      if (error.code) throw error;
      const mkdirError = new Error(
        `ENOENT: no such file or directory, mkdir '${path}'`
      );
      mkdirError.code = "ENOENT";
      mkdirError.syscall = "mkdir";
      mkdirError.path = path;
      throw mkdirError;
    }
  },
  readdirSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    if (typeof options === "string") options = { encoding: options };
    if (
      options?.encoding !== undefined &&
      !NodeBuffer.isEncoding(options.encoding)
    ) {
      throw Object.assign(new TypeError(`The argument 'encoding' is invalid. Received '${options.encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
    }
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
    if (options?.encoding === "hex") {
      return entries.map((name) => NodeBuffer.from(name));
    }
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
  rmdirSync: (value, options = {}) => {
    if (options?.recursive === true) {
      throw Object.assign(new TypeError("The recursive option is no longer supported for fs.rmdir"), { code: "ERR_INVALID_ARG_VALUE" });
    }
    const path = nodeFsPath(value);
    try {
      return globalThis.__quench_fs_remove_dir(path);
    } catch (error) {
      const message = String(error?.message || error);
      const code = message.startsWith("EACCES:")
        ? "EACCES"
        : message.includes("Not a directory")
          ? "ENOTDIR"
          : message.includes("No such file")
            ? "ENOENT"
            : message.includes("Directory not empty")
              ? "ENOTEMPTY"
              : undefined;
      if (code) {
        error.code = code;
        error.syscall = "rmdir";
        error.path = path;
      }
      throw error;
    }
  },
  renameSync: (from, to) =>
    globalThis.__quench_fs_rename(nodeFsPath(from), nodeFsPath(to)),
  unlinkSync: (value) => globalThis.__quench_fs_unlink(String(value)),
  truncateSync: (value, length = 0) => {
    if (typeof length !== "number" || !Number.isFinite(length)) {
      throw Object.assign(new TypeError('The "len" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (!Number.isInteger(length)) {
      throw Object.assign(new RangeError('The value of "len" is out of range'), { code: "ERR_OUT_OF_RANGE" });
    }
    const path =
      typeof value === "number"
        ? globalThis.__nodeFdPaths[value]
        : nodeFsPath(value);
    if (!path) throw new Error("EBADF");
    return globalThis.__quench_fs_truncate(path, Math.max(0, Number(length)));
  }
});
const __nodeGetPrototypeOf = Object.getPrototypeOf;
const __nodeFsConstants = globalThis.__nodeFs.constants;
Object.getPrototypeOf = (value) =>
  value === __nodeFsConstants ? null : __nodeGetPrototypeOf(value);
"#;
