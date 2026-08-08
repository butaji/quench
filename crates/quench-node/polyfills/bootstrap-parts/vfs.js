// Experimental node:vfs surface.  Keep the provider contract small and
// readable; mount interception is added only once the core tree semantics are
// covered by focused fixtures.
class __QuenchVirtualProvider {
  get readonly() {
    return false;
  }
  get supportsSymlinks() {
    return false;
  }
  get supportsWatch() {
    return false;
  }
  existsSync() {
    return false;
  }
  async open(path, flags = "r") {
    if (
      this instanceof __QuenchRealFSProvider &&
      !globalThis.__nodeFs.existsSync(
        globalThis.__nodePath.join(this.root, String(path))
      )
    )
      throw __quenchVfsError("ENOENT", "open", path);
    return { path, flags };
  }
  __notImplemented() {
    const error = new Error("Method not implemented");
    error.code = "ERR_METHOD_NOT_IMPLEMENTED";
    throw error;
  }
  __writeCheck() {
    if (this.readonly) {
      const error = new Error("Read-only file system");
      error.code = "EROFS";
      throw error;
    }
    return this.__notImplemented();
  }
  openSync() {
    return this.__notImplemented();
  }
  statSync() {
    return this.__notImplemented();
  }
  readdirSync() {
    return this.__notImplemented();
  }
  mkdirSync() {
    return this.__writeCheck();
  }
  rmdirSync() {
    return this.__writeCheck();
  }
  unlinkSync() {
    return this.__writeCheck();
  }
  renameSync() {
    return this.__writeCheck();
  }
  linkSync() {
    return this.__writeCheck();
  }
  readlinkSync() {
    return this.__notImplemented();
  }
  symlinkSync() {
    return this.__writeCheck();
  }
  watch() {
    return this.__notImplemented();
  }
  watchAsync() {
    return this.__notImplemented();
  }
  watchFile() {
    return this.__notImplemented();
  }
  unwatchFile() {
    return this.__notImplemented();
  }
  async open() {
    return this.__notImplemented();
  }
  async stat() {
    return this.__notImplemented();
  }
  async readdir() {
    return this.__notImplemented();
  }
  async mkdir() {
    return this.__writeCheck();
  }
  async rmdir() {
    return this.__writeCheck();
  }
  async unlink() {
    return this.__writeCheck();
  }
  async rename() {
    return this.__writeCheck();
  }
  async link() {
    return this.__writeCheck();
  }
  async readlink() {
    return this.__notImplemented();
  }
  async symlink() {
    return this.__writeCheck();
  }
  copyFileSync() {
    return this.__writeCheck();
  }
  writeFileSync() {
    return this.__writeCheck();
  }
  appendFileSync() {
    return this.__writeCheck();
  }
  async copyFile() {
    return this.__writeCheck();
  }
  async writeFile() {
    return this.__writeCheck();
  }
  async appendFile() {
    return this.__writeCheck();
  }
  lstatSync(path) {
    return this.statSync(path);
  }
  lstat(path) {
    return this.stat(path);
  }
  exists(path) {
    return Promise.resolve(this.existsSync(path));
  }
}
class __QuenchMemoryProvider extends __QuenchVirtualProvider {
  get supportsSymlinks() {
    return true;
  }
}
class __QuenchRealFSProvider extends __QuenchVirtualProvider {
  constructor(root = ".") {
    super();
    if (typeof root !== "string") {
      const error = new TypeError(
        "The rootPath argument must be of type string"
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    this.root = globalThis.__nodePath.resolve(root);
    this.rootPath = this.root;
  }
  get supportsSymlinks() {
    return true;
  }
  get supportsWatch() {
    return true;
  }
}

const __quenchVfsError = (code, syscall, path) => {
  const error = new Error(`${code}: ${syscall}, ${path}`);
  error.code = code;
  error.syscall = syscall;
  error.path = path;
  return error;
};
const __quenchVfsPath = (value) => {
  const path = globalThis.__nodePath.resolve(String(value));
  return path === "." ? "/" : path;
};
const __quenchVfsNormalizeFlags = (flags) => {
  if (typeof flags !== "number") return typeof flags === "string" ? flags : "r";
  const access = flags & 3;
  const append = (flags & 1024) !== 0;
  const create = (flags & 64) !== 0;
  const exclusive = (flags & 128) !== 0;
  const truncate = (flags & 512) !== 0;
  let result = access === 2 ? "r+" : access === 1 ? "w" : "r";
  if (append) result = access === 2 ? "a+" : "a";
  else if (truncate) result = access === 2 ? "w+" : "w";
  if (create && exclusive) result = result.replace(/^w/, "wx");
  else if (create && result === "r") result = "w";
  return result;
};
const __quenchVfsResolvePath = (entries, path, seen = []) => {
  const key = __quenchVfsPath(path);
  const parts = key.split("/").filter(Boolean);
  let current = "/";
  for (let index = 0; index < parts.length; index++) {
    const candidate =
      current === "/" ? `/${parts[index]}` : `${current}/${parts[index]}`;
    const entry = entries.get(candidate);
    if (entry?.type !== "symlink") {
      current = candidate;
      continue;
    }
    if (seen.includes(candidate)) {
      throw __quenchVfsError("ELOOP", "stat", path);
    }
    const target = entry.target.startsWith("/")
      ? entry.target
      : `${current}/${entry.target}`;
    const remainder = parts.slice(index + 1).join("/");
    return __quenchVfsResolvePath(
      entries,
      remainder ? `${target}/${remainder}` : target,
      [...seen, candidate]
    );
  }
  return current;
};
const __quenchVfsResolveParentPath = (entries, path) => {
  const key = __quenchVfsPath(path);
  const slash = key.lastIndexOf("/");
  const parent = slash <= 0 ? "/" : key.slice(0, slash);
  const name = key.slice(slash + 1);
  const resolvedParent = __quenchVfsResolvePath(entries, parent);
  return `${resolvedParent === "/" ? "" : resolvedParent}/${name}`;
};
class __QuenchVirtualFileSystem {
  constructor(provider, options) {
    if (
      provider !== undefined &&
      !(provider instanceof __QuenchVirtualProvider)
    ) {
      options = provider;
      provider = undefined;
    }
    if (
      options?.emitExperimentalWarning !== undefined &&
      typeof options.emitExperimentalWarning !== "boolean"
    ) {
      const error = new TypeError(
        'The "options.emitExperimentalWarning" property must be of type boolean'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    this.provider = provider || new __QuenchMemoryProvider();
    this.readonly = false;
    this.mountPoint = null;
    this.__entries = new Map([["/", { type: "dir", children: new Set() }]]);
    this.__fds = new Map();
    this.__closedFds = new Set();
    this.__realFds = new Set();
    this.__fdPositions = new Map();
    this.provider.open = async (path, flags = "r") =>
      this.__makeHandle(path, flags);
    const providerPath = (path) =>
      String(path).startsWith("/") ? path : `/${String(path)}`;
    this.provider.existsSync = (path) => !!this.__entry(providerPath(path));
    this.provider.copyFileSync = (source, destination, flags) =>
      this.copyFileSync(providerPath(source), providerPath(destination), flags);
    this.provider.copyFile = (source, destination, flags) =>
      this.copyFile(providerPath(source), providerPath(destination), flags);
    this.provider.accessSync = (path, mode) =>
      this.accessSync(providerPath(path), mode);
    this.promises = Object.fromEntries(
      [
        "readFile",
        "writeFile",
        "appendFile",
        "stat",
        "lstat",
        "readdir",
        "mkdir",
        "rmdir",
        "unlink",
        "rename",
        "copyFile",
        "rm",
        "realpath",
        "readlink",
        "symlink",
        "link",
        "truncate",
        "chmod",
        "chown",
        "lchown",
        "utimes",
        "lutimes",
        "statfs"
      ].map((name) => [name, async (...args) => this[`${name}Sync`](...args)])
    );
    this.promises.access = async (path, mode = 0) =>
      this.accessSync(path, mode);
    this.promises.open = async (path, flags = "r") =>
      this.openSync(path, flags);
  }
  __entry(path) {
    return this.__entries.get(__quenchVfsResolvePath(this.__entries, path));
  }
  __makeHandle(path, flags) {
    flags = __quenchVfsNormalizeFlags(flags);
    if (this.provider instanceof __QuenchRealFSProvider) {
      const fd = this.openSync(path, flags);
      let closed = false;
      const check = () => {
        if (closed) throw __quenchVfsError("EBADF", "read", path);
      };
      const readFileSync = (options) => {
        check();
        const bytes = globalThis.__quench_fs_native_read_all(fd);
        return options === "utf8" || options?.encoding
          ? globalThis.Buffer.from(bytes).toString(
              options === "utf8" ? "utf8" : options.encoding
            )
          : globalThis.Buffer.from(bytes);
      };
      return {
        fd,
        get closed() {
          return closed;
        },
        readFileSync,
        readFile: async (options) => readFileSync(options),
        closeSync: () => {
          if (!closed) {
            closed = true;
            this.closeSync(fd);
          }
        },
        close: async () => {
          if (!closed) {
            closed = true;
            this.closeSync(fd);
          }
        }
      };
    }
    const key = __quenchVfsPath(path);
    const writable = /[wa+]/.test(String(flags));
    if (!this.__entry(key) && !writable) {
      throw __quenchVfsError("ENOENT", "open", path);
    }
    if (!this.__entry(key)) this.writeFileSync(key, "");
    if (String(flags).startsWith("w")) this.writeFileSync(key, "");
    const vfs = this;
    let position = 0;
    let closed = false;
    const check = (write = false) => {
      if (closed || (write && !writable) || (!write && String(flags) === "w")) {
        throw __quenchVfsError("EBADF", write ? "write" : "read", path);
      }
    };
    const handle = {
      path: key,
      flags: String(flags),
      mode: 0o666,
      get closed() {
        return closed;
      },
      readFileSync(options) {
        check();
        return vfs.readFileSync(key, options);
      },
      readFile(options) {
        return Promise.resolve().then(() => this.readFileSync(options));
      },
      writeFileSync(data) {
        check(true);
        vfs.writeFileSync(key, data);
      },
      writeFile(data) {
        return Promise.resolve().then(() => this.writeFileSync(data));
      },
      statSync() {
        check();
        return vfs.statSync(key);
      },
      stat() {
        return Promise.resolve().then(() => this.statSync());
      },
      readSync(buffer, offset = 0, length = buffer.length - offset, at = null) {
        check();
        const source = vfs.readFileSync(key);
        const start = at == null || at < 0 ? position : at;
        const count = Math.max(0, Math.min(length, source.length - start));
        buffer.set(
          globalThis.Buffer.from(source).subarray(start, start + count),
          offset
        );
        position = start + count;
        return count;
      },
      read(buffer, offset, length, at) {
        return Promise.resolve().then(() => ({
          bytesRead: this.readSync(buffer, offset, length, at),
          buffer
        }));
      },
      writeSync(
        buffer,
        offset = 0,
        length = buffer.length - offset,
        at = null
      ) {
        check(true);
        const data = vfs.readFileSync(key);
        const start = at == null || at < 0 ? position : at;
        const bytes = globalThis.Buffer.from(buffer).subarray(
          offset,
          offset + length
        );
        const next = globalThis.Buffer.from(data);
        const out = globalThis.Buffer.alloc(
          Math.max(next.length, start + bytes.length)
        );
        next.copy(out);
        bytes.copy(out, start);
        vfs.writeFileSync(key, out);
        position = start + bytes.length;
        return bytes.length;
      },
      write(buffer, offset, length, at) {
        return Promise.resolve().then(() => ({
          bytesWritten: this.writeSync(buffer, offset, length, at),
          buffer
        }));
      },
      readv(buffers, at) {
        return Promise.resolve().then(() => {
          let total = 0;
          for (const b of buffers) {
            const n = this.readSync(
              b,
              0,
              b.length,
              at == null ? null : at + total
            );
            total += n;
            if (n < b.length) break;
          }
          return { bytesRead: total, buffers };
        });
      },
      writev(buffers, at) {
        return Promise.resolve().then(() => {
          let total = 0;
          for (const b of buffers) {
            total += this.writeSync(
              b,
              0,
              b.length,
              at == null ? null : at + total
            );
          }
          return { bytesWritten: total, buffers };
        });
      },
      appendFile(data) {
        return Promise.resolve().then(() => {
          check(true);
          vfs.appendFileSync(key, data);
        });
      },
      truncateSync(length = 0) {
        check(true);
        const data = vfs.readFileSync(key);
        const size = Math.max(0, Number(length));
        const out = globalThis.Buffer.alloc(size);
        globalThis.Buffer.from(data).subarray(0, size).copy(out);
        vfs.writeFileSync(key, out);
      },
      truncate(length) {
        return Promise.resolve().then(() => this.truncateSync(length));
      },
      closeSync() {
        closed = true;
      },
      close() {
        closed = true;
        return Promise.resolve();
      },
      chmod() {
        return Promise.resolve();
      },
      chown() {
        return Promise.resolve();
      },
      utimes() {
        return Promise.resolve();
      },
      datasync() {
        return Promise.resolve();
      },
      sync() {
        return Promise.resolve();
      },
      readableWebStream() {
        const e = new Error("Method not implemented");
        e.code = "ERR_METHOD_NOT_IMPLEMENTED";
        throw e;
      },
      readLines() {
        const e = new Error("Method not implemented");
        e.code = "ERR_METHOD_NOT_IMPLEMENTED";
        throw e;
      },
      createReadStream() {
        const e = new Error("Method not implemented");
        e.code = "ERR_METHOD_NOT_IMPLEMENTED";
        throw e;
      },
      createWriteStream() {
        const e = new Error("Method not implemented");
        e.code = "ERR_METHOD_NOT_IMPLEMENTED";
        throw e;
      }
    };
    Symbol.asyncDispose ||= Symbol("Symbol.asyncDispose");
    handle[Symbol.asyncDispose] = handle.close;
    return handle;
  }
  __realPath(path) {
    return globalThis.__nodePath.join(
      this.provider.root,
      __quenchVfsPath(path)
    );
  }
  __isReal() {
    return this.provider?.rootPath !== undefined;
  }
  existsSync(path) {
    try {
      if (this.__isReal()) {
        return globalThis.__nodeFs.existsSync(this.__realPath(path));
      }
      if (
        this.provider &&
        this.provider.existsSync !==
          __QuenchVirtualProvider.prototype.existsSync
      ) {
        return !!this.provider.existsSync(__quenchVfsPath(path));
      }
      return !!this.__entry(path);
    } catch {
      return false;
    }
  }
  accessSync(path, mode = 0) {
    const entry = this.__entry(path);
    if (!entry) {
      throw __quenchVfsError("ENOENT", "access", path);
    }
    const requested = mode == null ? 0 : Number(mode);
    const permissions = entry.mode ?? (entry.type === "dir" ? 0o755 : 0o666);
    if (
      (requested & 4 && !(permissions & 0o444)) ||
      (requested & 2 && !(permissions & 0o222)) ||
      (requested & 1 && !(permissions & 0o111))
    ) {
      throw __quenchVfsError("EACCES", "access", path);
    }
  }
  truncateSync(path, length = 0) {
    const entry = this.__entry(path);
    if (!entry) throw __quenchVfsError("ENOENT", "truncate", path);
    if (entry.type !== "file") {
      throw __quenchVfsError("EISDIR", "truncate", path);
    }
    const size = Math.max(0, Number(length));
    entry.data = globalThis.Buffer.from(entry.data)
      .subarray(0, size)
      .toString();
    if (entry.data.length < size) {
      entry.data += "\0".repeat(size - entry.data.length);
    }
  }
  chmodSync(path, mode) {
    const entry = this.__entry(path);
    if (!entry) throw __quenchVfsError("ENOENT", "chmod", path);
    entry.mode = Number(mode) & 0o777;
  }
  chownSync(path) {
    if (!this.__entry(path)) throw __quenchVfsError("ENOENT", "chown", path);
  }
  lchownSync(path) {
    if (!this.__entries.get(__quenchVfsPath(path))) {
      throw __quenchVfsError("ENOENT", "lchown", path);
    }
  }
  utimesSync(path) {
    if (!this.__entry(path)) throw __quenchVfsError("ENOENT", "utimes", path);
  }
  lutimesSync(path) {
    if (!this.__entries.get(__quenchVfsPath(path))) {
      throw __quenchVfsError("ENOENT", "lutimes", path);
    }
  }
  statSync(path, options = {}) {
    if (this.__isReal()) {
      return globalThis.__nodeFs.statSync(this.__realPath(path), options);
    }
    const entry = this.__entry(path);
    if (!entry) {
      if (options?.throwIfNoEntry === false) return undefined;
      throw __quenchVfsError("ENOENT", "stat", path);
    }
    return {
      size: options?.bigint
        ? BigInt(entry.type === "file" ? entry.data.length : 0)
        : entry.type === "file"
          ? entry.data.length
          : 0,
      mode: options?.bigint
        ? BigInt(
            entry.type === "dir"
              ? 0o40755
              : entry.type === "symlink"
                ? 0o120777
                : 0o100000 | (entry.mode ?? 0o666)
          )
        : entry.type === "dir"
          ? 0o40755
          : entry.type === "symlink"
            ? 0o120777
            : 0o100000 | (entry.mode ?? 0o666),
      ino: options?.bigint
        ? BigInt(__quenchVfsNextIno++)
        : __quenchVfsNextIno++,
      dev: options?.bigint ? 4085n : 4085,
      nlink: options?.bigint ? 1n : 1,
      uid: options?.bigint ? 0n : 0,
      gid: options?.bigint ? 0n : 0,
      isFile: () => entry.type === "file",
      isDirectory: () => entry.type === "dir",
      isSymbolicLink: () => false
    };
  }
  lstatSync(path, options) {
    if (this.__isReal()) {
      return globalThis.__nodeFs.lstatSync(this.__realPath(path), options);
    }
    const entry = this.__entries.get(__quenchVfsPath(path));
    if (!entry) {
      if (options?.throwIfNoEntry === false) return undefined;
      throw __quenchVfsError("ENOENT", "lstat", path);
    }
    return {
      size: options?.bigint
        ? BigInt(entry.type === "file" ? entry.data.length : 0)
        : entry.type === "file"
          ? entry.data.length
          : 0,
      mode: options?.bigint
        ? BigInt(
            entry.type === "dir"
              ? 0o40755
              : entry.type === "symlink"
                ? 0o120777
                : 0o100644
          )
        : entry.type === "dir"
          ? 0o40755
          : entry.type === "symlink"
            ? 0o120777
            : 0o100644,
      ino: options?.bigint
        ? BigInt(__quenchVfsNextIno++)
        : __quenchVfsNextIno++,
      dev: options?.bigint ? 4085n : 4085,
      nlink: options?.bigint ? 1n : 1,
      isFile: () => entry.type === "file",
      isDirectory: () => entry.type === "dir",
      isSymbolicLink: () => entry.type === "symlink"
    };
  }
  statfsSync(_path, options = {}) {
    const value = options.bigint ? 4096n : 4096;
    return { bsize: value, blocks: value, bfree: value, bavail: value };
  }
  mkdirSync(path, options = {}) {
    if (this.__isReal()) {
      return globalThis.__nodeFs.mkdirSync(this.__realPath(path), options);
    }
    const key = __quenchVfsPath(path);
    if (this.__entries.has(key)) {
      if (options.recursive) return undefined;
      throw __quenchVfsError("EEXIST", "mkdir", path);
    }
    const parent = key.slice(0, key.lastIndexOf("/")) || "/";
    if (!this.__entries.has(parent)) {
      if (!options.recursive) throw __quenchVfsError("ENOENT", "mkdir", path);
      this.mkdirSync(parent, { recursive: true });
    }
    this.__entries.set(key, { type: "dir", children: new Set() });
    return options.recursive ? path : undefined;
  }
  mkdtempSync(prefix, options = {}) {
    const base = __quenchVfsPath(prefix);
    const parent = base.slice(0, base.lastIndexOf("/")) || "/";
    if (!this.__entry(parent)) {
      throw __quenchVfsError("ENOENT", "mkdtemp", prefix);
    }
    const suffix = Math.random().toString(36).slice(2, 8).padEnd(6, "0");
    const result = `${base}${suffix}`;
    this.mkdirSync(result);
    return options?.encoding === "buffer"
      ? globalThis.Buffer.from(result)
      : result;
  }
  mkdtemp(prefix, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = {};
    }
    queueMicrotask(() => {
      try {
        callback(null, this.mkdtempSync(prefix, options));
      } catch (error) {
        callback(error);
      }
    });
  }
  writeFileSync(path, data) {
    if (
      this.provider instanceof __QuenchRealFSProvider &&
      typeof path === "number"
    ) {
      const bytes =
        typeof data === "string" ? globalThis.Buffer.from(data) : data;
      return globalThis.__quench_fs_native_write(path, Array.from(bytes));
    }
    if (
      this.provider instanceof __QuenchRealFSProvider &&
      typeof path !== "number"
    )
      return globalThis.__nodeFs.writeFileSync(this.__realPath(path), data);
    if (
      typeof path === "number" &&
      !(this.provider instanceof __QuenchRealFSProvider)
    ) {
      const key = this.__fds.get(path);
      const entry = key && this.__entries.get(key);
      if (!entry) throw __quenchVfsError("EBADF", "write", path);
      const text =
        typeof data === "string"
          ? data
          : data instanceof Uint8Array
            ? globalThis.Buffer.from(data).toString()
            : String(data);
      const offset = this.__fdPositions.get(path) || 0;
      entry.data = `${entry.data.slice(0, offset)}${text}${entry.data.slice(
        offset + text.length
      )}`;
      this.__fdPositions.set(path, offset + text.length);
      return;
    }
    const key =
      typeof path === "number"
        ? this.__fds.get(path)
        : __quenchVfsResolveParentPath(this.__entries, path);
    if (!key) throw __quenchVfsError("EBADF", "write", path);
    const parent = key.slice(0, key.lastIndexOf("/")) || "/";
    const parentEntry = this.__entries.get(parent);
    if (!parentEntry) throw __quenchVfsError("ENOENT", "open", path);
    if (parentEntry.type !== "dir") {
      throw __quenchVfsError("ENOTDIR", "open", path);
    }
    this.__entries.set(key, {
      type: "file",
      data:
        typeof data === "string"
          ? data
          : data instanceof Uint8Array
            ? globalThis.Buffer.from(data).toString()
            : String(data),
      mode: 0o666
    });
  }
  appendFileSync(path, data) {
    if (
      this.provider instanceof __QuenchRealFSProvider &&
      typeof path !== "number"
    )
      return globalThis.__nodeFs.appendFileSync(this.__realPath(path), data);
    const current = this.existsSync(path)
      ? this.readFileSync(path, "utf8")
      : "";
    this.writeFileSync(
      path,
      current + (typeof data === "string" ? data : String(data))
    );
  }
  readFileSync(path, options) {
    if (
      this.provider instanceof __QuenchRealFSProvider &&
      typeof path === "number"
    ) {
      const bytes = globalThis.__quench_fs_native_read_all(path);
      return options === "utf8" || options?.encoding
        ? globalThis.Buffer.from(bytes).toString(
            options === "utf8" ? "utf8" : options.encoding
          )
        : globalThis.Buffer.from(bytes);
    }
    if (
      this.provider instanceof __QuenchRealFSProvider &&
      typeof path !== "number"
    )
      return globalThis.__nodeFs.readFileSync(this.__realPath(path), options);
    const key = typeof path === "number" ? this.__fds.get(path) : path;
    const entry = this.__entry(key);
    if (!entry) throw __quenchVfsError("ENOENT", "open", path);
    if (entry.type !== "file") throw __quenchVfsError("EISDIR", "read", path);
    return options === "utf8" || options?.encoding
      ? entry.data
      : globalThis.Buffer.from(entry.data);
  }
  readFile(path, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    queueMicrotask(() => {
      try {
        callback(null, this.readFileSync(path, options));
      } catch (error) {
        callback(error);
      }
    });
  }
  writeFile(path, data, options, callback) {
    if (typeof options === "function") callback = options;
    queueMicrotask(() => {
      try {
        this.writeFileSync(path, data);
        callback(null);
      } catch (error) {
        callback(error);
      }
    });
  }
  appendFile(path, data, options, callback) {
    if (typeof options === "function") callback = options;
    queueMicrotask(() => {
      try {
        this.appendFileSync(path, data);
        callback(null);
      } catch (error) {
        callback(error);
      }
    });
  }
  openSync(path, flags = "r") {
    flags = __quenchVfsNormalizeFlags(flags);
    if (this.provider instanceof __QuenchRealFSProvider) {
      const fd = globalThis.__quench_fs_native_open(
        this.__realPath(path),
        String(flags)
      );
      this.__realFds.add(fd);
      this.__fds.set(fd, this.__realPath(path));
      return fd;
    }
    const key = __quenchVfsPath(path);
    if (this.__entry(key) && String(flags).includes("x")) {
      throw __quenchVfsError("EEXIST", "open", path);
    }
    if (
      !this.__entry(key) &&
      !String(flags).includes("w") &&
      !String(flags).includes("a")
    )
      throw __quenchVfsError("ENOENT", "open", path);
    if (
      !this.__entry(key) &&
      (String(flags).includes("w") || String(flags).includes("a"))
    )
      this.writeFileSync(key, "");
    const fd = __quenchVfsNextFd++;
    this.__fds.set(fd, key);
    globalThis.__quenchVfsFdHandles ||= new Map();
    globalThis.__quenchVfsFdHandles.set(fd, {
      entry: this.__makeHandle(key, flags)
    });
    this.__fdPositions.set(
      fd,
      String(flags).includes("a") ? this.readFileSync(key).length : 0
    );
    return fd;
  }
  readSync(fd, buffer, offset, length, position) {
    const key = this.__fds.get(fd);
    if (!key) throw __quenchVfsError("EBADF", "read", fd);
    const at =
      position == null || position < 0
        ? this.__fdPositions.get(fd) || 0
        : position;
    const result = this.__makeHandle(key, "r").readSync(
      buffer,
      offset,
      length,
      at
    );
    this.__fdPositions.set(fd, at + result);
    return result;
  }
  writeSync(fd, buffer, offset, length, position) {
    const key = this.__fds.get(fd);
    if (!key) throw __quenchVfsError("EBADF", "write", fd);
    if (typeof buffer === "string") {
      position = offset;
      buffer = globalThis.Buffer.from(
        buffer,
        typeof length === "string" ? length : "utf8"
      );
      offset = 0;
      length = buffer.length;
    }
    const at =
      position == null || position < 0
        ? this.__fdPositions.get(fd) || 0
        : position;
    const result = this.__makeHandle(key, "r+").writeSync(
      buffer,
      offset,
      length,
      at
    );
    this.__fdPositions.set(fd, at + result);
    return result;
  }
  fstatSync(fd) {
    const key = this.__fds.get(fd);
    if (!key) throw __quenchVfsError("EBADF", "fstat", fd);
    return this.statSync(key);
  }
  ftruncateSync(fd, length = 0) {
    const key = this.__fds.get(fd);
    if (!key) throw __quenchVfsError("EBADF", "ftruncate", fd);
    const data = this.readFileSync(key);
    const out = globalThis.Buffer.alloc(Math.max(0, length));
    globalThis.Buffer.from(data).subarray(0, length).copy(out);
    this.writeFileSync(key, out);
  }
  readvSync(fd, buffers, position = null) {
    let total = 0;
    for (const buffer of buffers) {
      const count = this.readSync(
        fd,
        buffer,
        0,
        buffer.length,
        position == null ? null : position + total
      );
      total += count;
      if (count < buffer.length) break;
    }
    return total;
  }
  writevSync(fd, buffers, position = null) {
    let total = 0;
    for (const buffer of buffers) {
      total += this.writeSync(
        fd,
        buffer,
        0,
        buffer.length,
        position == null ? null : position + total
      );
    }
    return total;
  }
  fchmodSync(fd) {
    if (!this.__fds.has(fd)) throw __quenchVfsError("EBADF", "fchmod", fd);
  }
  fchownSync(fd) {
    if (!this.__fds.has(fd)) throw __quenchVfsError("EBADF", "fchown", fd);
  }
  futimesSync(fd) {
    if (!this.__fds.has(fd)) throw __quenchVfsError("EBADF", "futimes", fd);
  }
  fsyncSync(fd) {
    if (!this.__fds.has(fd)) throw __quenchVfsError("EBADF", "fsync", fd);
  }
  fdatasyncSync(fd) {
    return this.fsyncSync(fd);
  }
  closeSync(fd) {
    if (this.__realFds.delete(fd)) {
      this.__fds.delete(fd);
      return globalThis.__quench_fs_native_close(fd);
    }
    if (!this.__fds.delete(fd)) throw __quenchVfsError("EBADF", "close", fd);
    this.__closedFds.add(fd);
    this.__fdPositions.delete(fd);
    globalThis.__quenchVfsFdHandles?.delete(fd);
  }
  readdirSync(path, options = {}) {
    if (this.__isReal()) {
      return globalThis.__nodeFs.readdirSync(this.__realPath(path), options);
    }
    const key = __quenchVfsPath(path);
    if (!this.__entries.has(key)) {
      throw __quenchVfsError("ENOENT", "scandir", path);
    }
    if (this.__entries.get(key).type !== "dir") {
      throw __quenchVfsError("ENOTDIR", "scandir", path);
    }
    const prefix = key === "/" ? "/" : `${key}/`;
    const names = [...this.__entries.keys()]
      .filter(
        (item) =>
          item !== key &&
          item.startsWith(prefix) &&
          !item.slice(prefix.length).includes("/")
      )
      .map((item) => item.slice(prefix.length));
    if (options.withFileTypes) {
      return names.map((name) => ({
        name,
        isFile: () => this.__entry(`${key}/${name}`).type === "file",
        isDirectory: () => this.__entry(`${key}/${name}`).type === "dir"
      }));
    }
    if (options.encoding === "buffer") {
      return names.map((name) => globalThis.Buffer.from(name));
    }
    return names;
  }
  opendirSync(path) {
    const key = __quenchVfsResolvePath(this.__entries, path);
    const entry = this.__entries.get(key);
    if (!entry) throw __quenchVfsError("ENOENT", "scandir", path);
    if (entry.type !== "dir") {
      throw __quenchVfsError("ENOTDIR", "scandir", path);
    }
    const prefix = key === "/" ? "/" : `${key}/`;
    const names = [...this.__entries.keys()]
      .filter(
        (item) =>
          item !== key &&
          item.startsWith(prefix) &&
          !item.slice(prefix.length).includes("/")
      )
      .map((item) => item.slice(prefix.length));
    let index = 0;
    let closed = false;
    const closedError = () => {
      const error = new Error("Directory handle was closed");
      error.code = "ERR_DIR_CLOSED";
      return error;
    };
    const check = () => {
      if (closed) throw closedError();
    };
    const dir = {
      path: key,
      readSync: () => {
        check();
        const name = names[index++];
        if (name === undefined) return null;
        const item = this.__entries.get(`${key}/${name}`);
        return {
          name,
          isFile: () => item.type === "file",
          isDirectory: () => item.type === "dir",
          isSymbolicLink: () => item.type === "symlink"
        };
      },
      read: (callback) => {
        if (typeof callback === "function") {
          queueMicrotask(() => {
            try {
              callback(null, dir.readSync());
            } catch (error) {
              callback(error);
            }
          });
          return;
        }
        return Promise.resolve().then(() => dir.readSync());
      },
      closeSync: () => {
        if (closed) throw closedError();
        closed = true;
      },
      close: (callback) => {
        if (typeof callback === "function") {
          queueMicrotask(() => {
            try {
              dir.closeSync();
              callback(null);
            } catch (error) {
              callback(error);
            }
          });
          return;
        }
        return Promise.resolve().then(() => dir.closeSync());
      },
      entries: async function* () {
        while (true) {
          const item = dir.readSync();
          if (item === null) return;
          yield item;
        }
      },
      [Symbol.asyncIterator]() {
        return this.entries();
      },
      [Symbol.dispose]() {
        if (!closed) closed = true;
      }
    };
    return dir;
  }
  opendir(path, options, callback) {
    if (typeof options === "function") callback = options;
    queueMicrotask(() => {
      try {
        callback(null, this.opendirSync(path));
      } catch (error) {
        callback(error);
      }
    });
  }
  unlinkSync(path) {
    if (this.provider instanceof __QuenchRealFSProvider) {
      return globalThis.__nodeFs.unlinkSync(this.__realPath(path));
    }
    const key = __quenchVfsPath(path);
    const entry = this.__entries.get(key);
    if (!entry) throw __quenchVfsError("ENOENT", "unlink", path);
    if (entry.type === "dir") throw __quenchVfsError("EISDIR", "unlink", path);
    this.__entries.delete(key);
  }
  symlinkSync(target, path) {
    if (this.__isReal()) {
      const destination = this.__realPath(path);
      const targetPath = String(target).startsWith("/")
        ? this.__realPath(target)
        : globalThis.__nodePath.resolve(
            globalThis.__nodePath.dirname(destination),
            String(target)
          );
      if (
        targetPath !== this.provider.root &&
        !targetPath.startsWith(
          `${this.provider.root}${globalThis.__nodePath.sep}`
        )
      )
        throw __quenchVfsError("EACCES", "symlink", path);
      return globalThis.__nodeFs.symlinkSync(
        String(target).startsWith("/") ? targetPath : target,
        destination
      );
    }
    const key = __quenchVfsPath(path);
    if (this.__entries.has(key)) {
      throw __quenchVfsError("EEXIST", "symlink", path);
    }
    const parent = key.slice(0, key.lastIndexOf("/")) || "/";
    if (!this.__entries.has(parent)) {
      throw __quenchVfsError("ENOENT", "symlink", path);
    }
    this.__entries.set(key, { type: "symlink", target: String(target) });
  }
  readlinkSync(path, options) {
    if (this.__isReal()) {
      const target = globalThis.__nodeFs.readlinkSync(
        this.__realPath(path),
        options
      );
      if (globalThis.__nodePath.isAbsolute(target)) {
        const resolved = globalThis.__nodePath.resolve(target);
        if (resolved === this.provider.root) return "/";
        if (
          resolved.startsWith(
            `${this.provider.root}${globalThis.__nodePath.sep}`
          )
        ) {
          const relative = resolved.slice(this.provider.root.length);
          return options?.encoding === "buffer"
            ? globalThis.Buffer.from(relative)
            : relative;
        }
      }
      return target;
    }
    const entry = this.__entries.get(__quenchVfsPath(path));
    if (!entry) throw __quenchVfsError("ENOENT", "readlink", path);
    if (entry.type !== "symlink") {
      throw __quenchVfsError("EINVAL", "readlink", path);
    }
    const target = entry.target;
    return options?.encoding === "buffer"
      ? globalThis.Buffer.from(target)
      : target;
  }
  linkSync(existingPath, newPath) {
    const source = this.__entry(existingPath);
    const target = __quenchVfsPath(newPath);
    if (!source) throw __quenchVfsError("ENOENT", "link", existingPath);
    if (source.type === "dir") {
      throw __quenchVfsError("EINVAL", "link", existingPath);
    }
    if (this.__entries.has(target)) {
      throw __quenchVfsError("EEXIST", "link", newPath);
    }
    this.__entries.set(target, source);
  }
  renameSync(oldPath, newPath) {
    if (this.provider instanceof __QuenchRealFSProvider) {
      return globalThis.__nodeFs.renameSync(
        this.__realPath(oldPath),
        this.__realPath(newPath)
      );
    }
    const oldKey = __quenchVfsPath(oldPath);
    const newKey = __quenchVfsPath(newPath);
    const entry = this.__entries.get(oldKey);
    if (!entry) throw __quenchVfsError("ENOENT", "rename", oldPath);
    const existing = this.__entries.get(newKey);
    if (existing && entry.type === "file" && existing.type === "dir") {
      throw __quenchVfsError("EISDIR", "rename", newPath);
    }
    if (existing && entry.type === "dir" && existing.type === "file") {
      throw __quenchVfsError("ENOTDIR", "rename", newPath);
    }
    if (entry.type === "dir" && newKey.startsWith(`${oldKey}/`)) {
      throw __quenchVfsError("EINVAL", "rename", newPath);
    }
    if (existing) this.__entries.delete(newKey);
    this.__entries.set(newKey, entry);
    this.__entries.delete(oldKey);
  }
  rmSync(path, options = {}) {
    if (this.provider instanceof __QuenchRealFSProvider) {
      return globalThis.__nodeFs.rmSync(this.__realPath(path), options);
    }
    const key = __quenchVfsPath(path);
    if (!this.__entries.has(key)) {
      if (options.force) return;
      throw __quenchVfsError("ENOENT", "rm", path);
    }
    for (const child of [...this.__entries.keys()]) {
      if (child === key || child.startsWith(`${key}/`)) {
        this.__entries.delete(child);
      }
    }
  }
  rmdirSync(path) {
    if (this.provider instanceof __QuenchRealFSProvider) {
      return globalThis.__nodeFs.rmdirSync(this.__realPath(path));
    }
    const key = __quenchVfsPath(path);
    const entry = this.__entries.get(key);
    if (!entry) throw __quenchVfsError("ENOENT", "rmdir", path);
    if (entry.type !== "dir") throw __quenchVfsError("ENOTDIR", "rmdir", path);
    if (
      [...this.__entries.keys()].some(
        (item) => item !== key && item.startsWith(`${key}/`)
      )
    )
      throw __quenchVfsError("ENOTEMPTY", "rmdir", path);
    this.__entries.delete(key);
  }
  copyFileSync(source, destination, mode = 0) {
    if (this.__isReal()) {
      return globalThis.__nodeFs.copyFileSync(
        this.__realPath(source),
        this.__realPath(destination),
        mode
      );
    }
    if (mode & 1 && this.existsSync(destination)) {
      throw __quenchVfsError("EEXIST", "copyfile", destination);
    }
    const data = this.readFileSync(source);
    this.writeFileSync(destination, data);
  }
  realpathSync(path) {
    if (this.provider instanceof __QuenchRealFSProvider) {
      const resolved = globalThis.__nodeFs.realpathSync(this.__realPath(path));
      if (
        resolved !== this.provider.root &&
        !resolved.startsWith(
          `${this.provider.root}${globalThis.__nodePath.sep}`
        )
      )
        throw __quenchVfsError("EACCES", "realpath", path);
      const result = resolved.slice(this.provider.root.length) || "/";
      return arguments[1]?.encoding === "buffer"
        ? globalThis.Buffer.from(result)
        : result;
    }
    if (!this.__entry(path)) throw __quenchVfsError("ENOENT", "realpath", path);
    const result = __quenchVfsPath(path);
    return arguments[1]?.encoding === "buffer"
      ? globalThis.Buffer.from(result)
      : result;
  }
  mount(path) {
    if (this.mountPoint) {
      const error = new Error("VFS is already mounted");
      error.code = "ERR_INVALID_STATE";
      throw error;
    }
    const mountPoint = __quenchVfsPath(path);
    for (const other of __quenchVfsMounts) {
      if (
        other.mountPoint === mountPoint ||
        other.mountPoint?.startsWith(`${mountPoint}/`) ||
        mountPoint.startsWith(`${other.mountPoint}/`)
      ) {
        const error = new Error("VFS mount points overlap");
        error.code = "ERR_INVALID_STATE";
        throw error;
      }
    }
    this.mountPoint = mountPoint;
    __quenchVfsMounts.add(this);
    return this;
  }
  unmount() {
    __quenchVfsMounts.delete(this);
    this.mountPoint = null;
  }
  get mounted() {
    return this.mountPoint !== null;
  }
  [Symbol.dispose]() {
    this.unmount();
  }
  shouldHandle(path) {
    const key = __quenchVfsPath(path);
    return (
      !!this.mountPoint &&
      (key === this.mountPoint || key.startsWith(`${this.mountPoint}/`))
    );
  }
}
const __quenchVfsCallback = (vfs, callback, operation) => {
  queueMicrotask(() => {
    try {
      callback(null, operation());
    } catch (error) {
      callback(error);
    }
  });
};
__QuenchVirtualFileSystem.prototype.stat = function (path, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  __quenchVfsCallback(this, callback, () => this.statSync(path, options));
};
__QuenchVirtualFileSystem.prototype.lstat = function (path, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  __quenchVfsCallback(this, callback, () => this.lstatSync(path, options));
};
__QuenchVirtualFileSystem.prototype.readdir = function (
  path,
  options,
  callback
) {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  __quenchVfsCallback(this, callback, () => this.readdirSync(path, options));
};
__QuenchVirtualFileSystem.prototype.realpath = function (
  path,
  options,
  callback
) {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  __quenchVfsCallback(this, callback, () => this.realpathSync(path, options));
};
__QuenchVirtualFileSystem.prototype.access = function (path, mode, callback) {
  if (typeof mode === "function") {
    callback = mode;
    mode = 0;
  }
  __quenchVfsCallback(this, callback, () => this.accessSync(path, mode));
};
__QuenchVirtualFileSystem.prototype.rm = function (path, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  __quenchVfsCallback(this, callback, () => this.rmSync(path, options));
};
__QuenchVirtualFileSystem.prototype.truncate = function (
  path,
  length,
  callback
) {
  if (typeof length === "function") {
    callback = length;
    length = 0;
  }
  __quenchVfsCallback(this, callback, () => this.truncateSync(path, length));
};
__QuenchVirtualFileSystem.prototype.link = function (source, target, callback) {
  __quenchVfsCallback(this, callback, () => this.linkSync(source, target));
};
__QuenchVirtualFileSystem.prototype.symlink = function (
  target,
  path,
  callback
) {
  __quenchVfsCallback(this, callback, () => this.symlinkSync(target, path));
};
__QuenchVirtualFileSystem.prototype.readlink = function (
  path,
  options,
  callback
) {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  __quenchVfsCallback(this, callback, () => this.readlinkSync(path, options));
};
__QuenchVirtualFileSystem.prototype.open = function (
  path,
  flags,
  mode,
  callback
) {
  if (typeof flags === "function") {
    callback = flags;
    flags = "r";
  } else if (typeof mode === "function") callback = mode;
  __quenchVfsCallback(this, callback, () => this.openSync(path, flags));
};
__QuenchVirtualFileSystem.prototype.fstat = function (fd, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  __quenchVfsCallback(this, callback, () => this.fstatSync(fd, options));
};
__QuenchVirtualFileSystem.prototype.ftruncate = function (
  fd,
  length,
  callback
) {
  if (typeof length === "function") {
    callback = length;
    length = 0;
  }
  __quenchVfsCallback(this, callback, () => this.ftruncateSync(fd, length));
};
__QuenchVirtualFileSystem.prototype.read = function (
  fd,
  buffer,
  offset,
  length,
  position,
  callback
) {
  if (typeof position === "function") {
    callback = position;
    position = null;
  }
  __quenchVfsCallback(this, callback, () =>
    this.readSync(fd, buffer, offset, length, position)
  );
};
__QuenchVirtualFileSystem.prototype.write = function (
  fd,
  buffer,
  offset,
  length,
  position,
  callback
) {
  if (typeof position === "function") {
    callback = position;
    position = null;
  }
  __quenchVfsCallback(this, callback, () =>
    this.writeSync(fd, buffer, offset, length, position)
  );
};
__QuenchVirtualFileSystem.prototype.close = function (fd, callback) {
  __quenchVfsCallback(this, callback, () => this.closeSync(fd));
};
const __quenchVfsMounts = new Set();
let __quenchVfsNextFd = 0x40000000;
let __quenchVfsNextIno = 1;
const __quenchVfsStats = (kind, size = 0, options = {}) => {
  const bigint = options?.bigint === true;
  const value = (number) => (bigint ? BigInt(number) : number);
  const mode =
    kind === "zero"
      ? 0
      : kind === "file"
        ? 0o644
        : kind === "dir"
          ? 0o755
          : 0o777;
  return {
    size: value(size),
    mode: value(mode),
    nlink: value(1),
    uid: value(0),
    gid: value(0),
    ino: value(__quenchVfsNextIno++),
    dev: value(4085),
    isFile: () => kind === "file",
    isDirectory: () => kind === "dir",
    isSymbolicLink: () => kind === "symlink"
  };
};
globalThis.__quenchVfsStatsHelpers = {
  createFileStats: (size = 0, options = {}) =>
    __quenchVfsStats("file", size, options),
  createDirectoryStats: (options = {}) => __quenchVfsStats("dir", 0, options),
  createSymlinkStats: (size = 0, options = {}) =>
    __quenchVfsStats("symlink", size, options),
  createZeroStats: (options = {}) => __quenchVfsStats("zero", 0, options)
};
const __quenchVfsRelative = (vfs, path) => {
  const key = __quenchVfsPath(path);
  return key === vfs.mountPoint ? "/" : key.slice(vfs.mountPoint.length);
};
const __quenchVfsWrapFs = (name, fallback) => {
  const original = globalThis.__nodeFs?.[name];
  if (typeof original !== "function") return;
  const wrapped = function (path, ...args) {
    if (typeof path === "number") {
      for (const vfs of __quenchVfsMounts) {
        if (vfs.__fds.has(path)) return vfs[name](path, ...args);
      }
    }
    for (const vfs of [...__quenchVfsMounts].reverse()) {
      const destination = args[0];
      if (
        (name === "symlinkSync" || name === "linkSync") &&
        vfs.shouldHandle(destination)
      ) {
        const source =
          name === "linkSync" ? __quenchVfsRelative(vfs, path) : path;
        return vfs[name](
          source,
          __quenchVfsRelative(vfs, destination),
          ...args.slice(1)
        );
      }
      if (
        (name === "renameSync" || name === "copyFileSync") &&
        typeof destination === "string" &&
        vfs.shouldHandle(path) &&
        vfs.shouldHandle(destination)
      ) {
        return vfs[name](
          __quenchVfsRelative(vfs, path),
          __quenchVfsRelative(vfs, destination),
          ...args.slice(1)
        );
      }
      if (vfs.shouldHandle(path)) {
        if (name === "copyFileSync" && typeof args[0] === "string") {
          args[0] = __quenchVfsRelative(vfs, args[0]);
        }
        const result = vfs[name](__quenchVfsRelative(vfs, path), ...args);
        if (name === "mkdtempSync" && typeof result === "string") {
          return `${vfs.mountPoint}${result}`;
        }
        if (name === "mkdtempSync" && globalThis.Buffer.isBuffer(result)) {
          return globalThis.Buffer.from(
            `${vfs.mountPoint}${result.toString()}`
          );
        }
        if (name === "realpathSync" && typeof result === "string") {
          return `${vfs.mountPoint}${result}`;
        }
        if (name === "realpathSync" && globalThis.Buffer.isBuffer(result)) {
          return globalThis.Buffer.from(
            `${vfs.mountPoint}${result.toString()}`
          );
        }
        return result;
      }
    }
    return original.call(this, path, ...args);
  };
  if (original.native) wrapped.native = original.native;
  globalThis.__nodeFs[name] = wrapped;
};
for (const __quenchVfsFsMethod of [
  "existsSync",
  "accessSync",
  "truncateSync",
  "chmodSync",
  "chownSync",
  "lchownSync",
  "utimesSync",
  "lutimesSync",
  "readFileSync",
  "writeFileSync",
  "appendFileSync",
  "statSync",
  "lstatSync",
  "statfsSync",
  "readdirSync",
  "opendirSync",
  "mkdirSync",
  "mkdtempSync",
  "unlinkSync",
  "symlinkSync",
  "readlinkSync",
  "linkSync",
  "renameSync",
  "rmSync",
  "rmdirSync",
  "copyFileSync",
  "realpathSync",
  "openSync",
  "closeSync",
  "readSync",
  "writeSync",
  "fstatSync",
  "ftruncateSync",
  "readvSync",
  "writevSync",
  "fchmodSync",
  "fchownSync",
  "futimesSync",
  "fsyncSync",
  "fdatasyncSync"
]) {
  __quenchVfsWrapFs(__quenchVfsFsMethod);
}
const __quenchVfsWrapAsyncFs = (name, syncName = `${name}Sync`) => {
  const original = globalThis.__nodeFs?.[name];
  if (typeof original !== "function") return;
  globalThis.__nodeFs[name] = function (path, ...args) {
    const vfs =
      typeof path === "number"
        ? [...__quenchVfsMounts]
            .reverse()
            .find((item) => item.__fds.has(path) || item.__closedFds.has(path))
        : [...__quenchVfsMounts]
            .reverse()
            .find((item) => item.shouldHandle(path));
    if (!vfs) return original.call(this, path, ...args);
    let callback = args.at(-1);
    if (typeof callback !== "function") {
      const target =
        typeof path === "number" ? path : __quenchVfsRelative(vfs, path);
      return Promise.resolve().then(() => {
        const result = vfs[syncName](target, ...args);
        if (name === "mkdtemp" && typeof result === "string") {
          return `${vfs.mountPoint}${result}`;
        }
        if (name === "readdir" && args[0]?.encoding === "buffer") {
          return result.map((item) => globalThis.Buffer.from(item));
        }
        if (name === "readlink" && args[0]?.encoding === "buffer") {
          return globalThis.Buffer.from(result);
        }
        return result;
      });
    }
    args.pop();
    queueMicrotask(() => {
      try {
        const target =
          typeof path === "number" ? path : __quenchVfsRelative(vfs, path);
        const result = vfs[syncName](target, ...args);
        const mountedResult =
          name === "mkdtemp" && typeof result === "string"
            ? `${vfs.mountPoint}${result}`
            : result;
        callback(null, mountedResult);
      } catch (error) {
        callback(error);
      }
    });
  };
};
for (const __quenchVfsAsyncMethod of [
  "access",
  "truncate",
  "chmod",
  "chown",
  "lchown",
  "utimes",
  "lutimes",
  "mkdir",
  "mkdtemp",
  "rmdir",
  "rm",
  "unlink",
  "copyFile",
  "readFile",
  "writeFile",
  "appendFile",
  "stat",
  "lstat",
  "readdir",
  "opendir",
  "open",
  "close",
  "ftruncate",
  "readlink"
])
  __quenchVfsWrapAsyncFs(__quenchVfsAsyncMethod);
for (const __quenchVfsTwoPathAsyncName of ["symlink", "link"]) {
  const original = globalThis.__nodeFs?.[__quenchVfsTwoPathAsyncName];
  if (typeof original !== "function") continue;
  globalThis.__nodeFs[__quenchVfsTwoPathAsyncName] = (
    source,
    destination,
    ...args
  ) => {
    const callback = args.at(-1);
    const vfs = [...__quenchVfsMounts]
      .reverse()
      .find((item) => item.shouldHandle(destination));
    if (!vfs || typeof callback !== "function") {
      return original(source, destination, ...args);
    }
    args.pop();
    queueMicrotask(() => {
      try {
        const sourcePath =
          __quenchVfsTwoPathAsyncName === "link"
            ? __quenchVfsRelative(vfs, source)
            : source;
        vfs[`${__quenchVfsTwoPathAsyncName}Sync`](
          sourcePath,
          __quenchVfsRelative(vfs, destination),
          ...args
        );
        callback(null);
      } catch (error) {
        callback(error);
      }
    });
  };
}
const __quenchVfsOriginalExists = globalThis.__nodeFs?.exists;
if (typeof __quenchVfsOriginalExists === "function") {
  globalThis.__nodeFs.exists = (path, callback) => {
    const vfs = [...__quenchVfsMounts]
      .reverse()
      .find((item) => item.shouldHandle(path));
    if (!vfs) return __quenchVfsOriginalExists(path, callback);
    queueMicrotask(() =>
      callback(vfs.existsSync(__quenchVfsRelative(vfs, path)))
    );
  };
}
const __quenchVfsOriginalRealpath = globalThis.__nodeFs?.realpath;
if (typeof __quenchVfsOriginalRealpath === "function") {
  globalThis.__nodeFs.realpath = (path, ...args) => {
    const callback = args.at(-1);
    const vfs = [...__quenchVfsMounts]
      .reverse()
      .find((item) => item.shouldHandle(path));
    if (!vfs || typeof callback !== "function") {
      return __quenchVfsOriginalRealpath(path, ...args);
    }
    queueMicrotask(() => {
      try {
        vfs.realpathSync(__quenchVfsRelative(vfs, path));
        callback(null, path);
      } catch (error) {
        callback(error);
      }
    });
  };
}
for (const [__quenchVfsAsyncFdName, __quenchVfsSyncFdName] of [
  ["read", "readSync"],
  ["write", "writeSync"],
  ["fstat", "fstatSync"]
]) {
  const __quenchVfsOriginalFd = globalThis.__nodeFs?.[__quenchVfsAsyncFdName];
  if (typeof __quenchVfsOriginalFd !== "function") continue;
  globalThis.__nodeFs[__quenchVfsAsyncFdName] = (...args) => {
    const fd = args[0];
    const vfs =
      [...__quenchVfsMounts].reverse().find((item) => item.__fds.has(fd)) ||
      [...__quenchVfsMounts].reverse().find((item) => item.__closedFds.has(fd));
    if (!vfs) return __quenchVfsOriginalFd(...args);
    const callback = args.pop();
    queueMicrotask(() => {
      try {
        const result = vfs[__quenchVfsSyncFdName](...args);
        if (__quenchVfsAsyncFdName === "fstat") callback(null, result);
        else callback(null, result, args[1]);
      } catch (error) {
        callback(error);
      }
    });
  };
}
const __quenchVfsPromiseNames = [
  "access",
  "appendFile",
  "chmod",
  "chown",
  "copyFile",
  "lchown",
  "lstat",
  "mkdir",
  "mkdtemp",
  "opendir",
  "readFile",
  "readlink",
  "readdir",
  "realpath",
  "rename",
  "rm",
  "rmdir",
  "stat",
  "statfs",
  "symlink",
  "truncate",
  "unlink",
  "utimes",
  "writeFile"
];
if (globalThis.__nodeFs?.promises) {
  for (const name of __quenchVfsPromiseNames) {
    const original = globalThis.__nodeFs.promises[name];
    if (typeof original !== "function") continue;
    globalThis.__nodeFs.promises[name] = (path, ...args) => {
      const routingPath =
        name === "symlink" || name === "link" ? args[0] : path;
      const vfs = [...__quenchVfsMounts]
        .reverse()
        .find((item) => item.shouldHandle(routingPath));
      if (!vfs) return original(path, ...args);
      const method = `${name}Sync`;
      if (name === "opendir") {
        return Promise.resolve(vfs.opendirSync(__quenchVfsRelative(vfs, path)));
      }
      if (name === "symlink" || name === "link") {
        const target = name === "link" ? __quenchVfsRelative(vfs, path) : path;
        return Promise.resolve(
          vfs[`${name}Sync`](target, __quenchVfsRelative(vfs, args[0]))
        );
      }
      return Promise.resolve().then(() => {
        const result = vfs[method](__quenchVfsRelative(vfs, path), ...args);
        if (name === "mkdtemp" && typeof result === "string") {
          return `${vfs.mountPoint}${result}`;
        }
        if (name === "readdir" && args[0]?.encoding === "buffer") {
          return result.map((item) => globalThis.Buffer.from(item));
        }
        if (name === "readlink" && args[0]?.encoding === "buffer") {
          return globalThis.Buffer.from(result);
        }
        return result;
      });
    };
  }
}
globalThis.__nodeVfs = {
  create(provider, options) {
    if (provider && !(provider instanceof __QuenchVirtualProvider)) {
      options = provider;
      provider = undefined;
    }
    return new __QuenchVirtualFileSystem(provider, options);
  },
  VirtualFileSystem: __QuenchVirtualFileSystem,
  VirtualProvider: __QuenchVirtualProvider,
  MemoryProvider: __QuenchMemoryProvider,
  RealFSProvider: __QuenchRealFSProvider
};
