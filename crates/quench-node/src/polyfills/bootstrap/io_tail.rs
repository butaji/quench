//! Polyfill: `io-tail`

pub const JS: &str = r#"Object.assign(globalThis.__nodeFs, {
  readlinkSync: (value, options) => {
    const encoding = typeof options === "string"
      ? options
      : options && options.encoding;
    if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
      throw Object.assign(new TypeError(`The argument 'encoding' is invalid. Received '${encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
    }
    const result = globalThis.__quench_fs_readlink(nodeFsPath(value));
    return encoding === "buffer"
      ? NodeBuffer.from(result)
      : encoding
      ? NodeBuffer.from(result).toString(encoding)
      : result;
  },
  chmodSync: (value, mode) => {
    const path = __nodeFsPathOnly(value);
    try {
      globalThis.__quench_fs_chmod(
        path,
        typeof mode === "string" ? parseInt(mode, 8) : Number(mode),
      );
    } catch (cause) {
      const error = new Error(
        `ENOENT: no such file or directory, chmod '${path}'`,
      );
      error.code = "ENOENT";
      error.syscall = "chmod";
      error.path = path;
      error.cause = cause;
      throw error;
    }
    globalThis.__nodeModes[path] = typeof mode === "string"
      ? parseInt(mode, 8)
      : Number(mode);
  },
  symlinkSync: (target, link, type) => {
    if (
      (typeof target !== "string" && !(target instanceof Uint8Array)) ||
      (typeof link !== "string" && !(link instanceof Uint8Array))
    ) {
      throw Object.assign(new TypeError('The "target" and "path" arguments must be strings or Buffer'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (type !== undefined && !["file", "dir", "junction"].includes(type)) {
      throw Object.assign(new TypeError('The "type" argument is invalid'), { code: "ERR_INVALID_ARG_VALUE" });
    }
    const targetPath = typeof target === "string"
      ? target
      : NodeBuffer.from(target).toString();
    return globalThis.__quench_fs_symlink(targetPath, nodeFsPath(link));
  },
  linkSync: (existing, link) => {
    if (
      (typeof existing !== "string" && !(existing instanceof Uint8Array)) ||
      (typeof link !== "string" && !(link instanceof Uint8Array))
    ) {
      throw Object.assign(new TypeError('The "path" argument must be of type string or an instance of Buffer or URL'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    return globalThis.__quench_fs_link(nodeFsPath(existing), nodeFsPath(link));
  },
});
"#;
