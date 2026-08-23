//! Polyfill: `dispatch`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchRequireParts = [
  globalThis.__quench_require_part_00,
  globalThis.__quench_require_part_01,
  globalThis.__quench_require_part_02,
  globalThis.__quench_require_part_03
];
globalThis.require = (specifier) => {
  const rawName = String(specifier);
  const name = rawName.startsWith("node:") ? rawName.slice(5) : rawName;
  if (
    name === "stream/iter" &&
    !globalThis.__quench_argv?.includes?.("--experimental-stream-iter")
  ) {
    throw Object.assign(
      new Error("No such built-in module: node:stream/iter"),
      { code: "ERR_UNKNOWN_BUILTIN_MODULE" }
    );
  }
  if (name === "internal/vfs/stats" && globalThis.__quenchVfsStatsHelpers) {
    return globalThis.__quenchVfsStatsHelpers;
  }
  if (name === "internal/vfs/fd") {
    return {
      getVirtualFd(fd) {
        return globalThis.__quenchVfsFdHandles?.get(fd);
      }
    };
  }
  if (name === "internal/timers") {
    return {
      setUnrefTimeout(callback, delay, ...args) {
        if (typeof callback !== "function") {
          throw Object.assign(
            new TypeError('The "callback" argument must be of type function'),
            { code: "ERR_INVALID_ARG_TYPE" }
          );
        }
        const timer = globalThis.setTimeout(callback, delay, ...args);
        timer.unref();
        return timer;
      }
    };
  }
  if (
    (rawName.startsWith(".") || rawName.startsWith("/")) &&
    typeof globalThis.__quenchLoadLocalModule === "function"
  ) {
    return globalThis.__quenchLoadLocalModule(
      rawName,
      globalThis.__quench_script_filename || globalThis.__filename
    );
  }
  if (name === "internal/vfs/router") {
    const path = globalThis.__nodePath;
    return {
      isUnderMountPoint(value, mountPoint) {
        const valuePath = path.resolve(value);
        const mountPath = path.resolve(mountPoint);
        return (
          mountPath === path.parse(mountPath).root ||
          valuePath === mountPath ||
          valuePath.startsWith(`${mountPath}${path.sep}`)
        );
      },
      getRelativePath(value, mountPoint) {
        const relative = path.relative(
          path.resolve(mountPoint),
          path.resolve(value)
        );
        return relative ? `/${relative.split(path.sep).join("/")}` : "/";
      },
      isAbsolutePath: path.isAbsolute
    };
  }
  if (name === "internal/vfs/file_handle") {
    class VirtualFileHandle {
      constructor(path, flags = "r", mode = 0o666) {
        this.path = path;
        this.flags = flags;
        this.mode = mode;
        this.position = 0;
        this.closed = false;
      }
      __check() {
        if (this.closed) {
          const error = new Error("file handle is closed");
          error.code = "EBADF";
          throw error;
        }
      }
      __stub() {
        this.__check();
        const error = new Error("Method not implemented");
        error.code = "ERR_METHOD_NOT_IMPLEMENTED";
        throw error;
      }
      closeSync() {
        this.closed = true;
      }
      async close() {
        this.closed = true;
      }
    }
    const defineHandleMethods = (names, implementation) => {
      for (const name of names.split(" ")) {
        const method = implementation();
        Object.defineProperty(method, "name", {
          configurable: true,
          value: name
        });
        VirtualFileHandle.prototype[name] = method;
      }
    };
    defineHandleMethods(
      "readSync writeSync readFileSync writeFileSync statSync truncateSync readableWebStream readLines createReadStream createWriteStream",
      function () {
        return function () {
          return this.__stub();
        };
      }
    );
    defineHandleMethods(
      "read write readFile writeFile stat truncate readv writev appendFile",
      function () {
        return function () {
          return Promise.reject().catch(() => this.__stub());
        };
      }
    );
    defineHandleMethods("chmod chown utimes datasync sync", function () {
      return async function () {};
    });
    Symbol.asyncDispose ||= Symbol("Symbol.asyncDispose");
    VirtualFileHandle.prototype[Symbol.asyncDispose] =
      VirtualFileHandle.prototype.close;
    class MemoryFileHandle extends VirtualFileHandle {
      constructor(path, flags = "r", mode = 0o666, content, getStats) {
        super(path, flags, mode);
        this.content = content;
        this.getStats = getStats;
      }
      statSync() {
        if (typeof this.getStats !== "function") {
          const error = new Error("File statistics are not available");
          error.code = "ERR_INVALID_STATE";
          throw error;
        }
        return this.getStats();
      }
    }
    return { MemoryFileHandle, VirtualFileHandle };
  }
  if (name === "worker_threads") {
    return { isMainThread: true, MessageChannel, MessagePort };
  }
  for (const handler of __quenchRequireParts) {
    const result = handler(name, specifier);
    if (result !== undefined) return result;
  }
  if (name.startsWith(".") || name.startsWith("/")) {
    return globalThis.__quenchLoadLocalModule(
      name,
      globalThis.__quench_script_filename || globalThis.__filename
    );
  }
  throw new Error("Cannot find module " + String(specifier));
};
"#);
