const __quenchRequireParts = [
  globalThis.__quench_require_part_00,
  globalThis.__quench_require_part_01,
  globalThis.__quench_require_part_02,
  globalThis.__quench_require_part_03
];
globalThis.require = (specifier) => {
  const rawName = String(specifier);
  const name = rawName.startsWith("node:") ? rawName.slice(5) : rawName;
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
      readSync() {
        return this.__stub();
      }
      writeSync() {
        return this.__stub();
      }
      readFileSync() {
        return this.__stub();
      }
      writeFileSync() {
        return this.__stub();
      }
      statSync() {
        return this.__stub();
      }
      truncateSync() {
        return this.__stub();
      }
      read() {
        return Promise.reject().catch(() => this.__stub());
      }
      write() {
        return Promise.reject().catch(() => this.__stub());
      }
      readFile() {
        return Promise.reject().catch(() => this.__stub());
      }
      writeFile() {
        return Promise.reject().catch(() => this.__stub());
      }
      stat() {
        return Promise.reject().catch(() => this.__stub());
      }
      truncate() {
        return Promise.reject().catch(() => this.__stub());
      }
      readv() {
        return Promise.reject().catch(() => this.__stub());
      }
      writev() {
        return Promise.reject().catch(() => this.__stub());
      }
      appendFile() {
        return Promise.reject().catch(() => this.__stub());
      }
      readableWebStream() {
        return this.__stub();
      }
      readLines() {
        return this.__stub();
      }
      createReadStream() {
        return this.__stub();
      }
      createWriteStream() {
        return this.__stub();
      }
      async chmod() {}
      async chown() {}
      async utimes() {}
      async datasync() {}
      async sync() {}
      closeSync() {
        this.closed = true;
      }
      async close() {
        this.closed = true;
      }
    }
    Symbol.asyncDispose ||= Symbol("Symbol.asyncDispose");
    VirtualFileHandle.prototype[Symbol.asyncDispose] =
      VirtualFileHandle.prototype.close;
    return { VirtualFileHandle };
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
