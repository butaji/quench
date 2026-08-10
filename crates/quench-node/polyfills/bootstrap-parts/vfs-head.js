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
    ) {
      throw __quenchVfsError("ENOENT", "open", path);
    }
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
  async open() {
    return this.__notImplemented();
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
const __quenchVfsMethods = (names, implementation) => {
  for (const name of names.split(" ")) {
    const method = implementation();
    Object.defineProperty(method, "name", { configurable: true, value: name });
    Object.defineProperty(__QuenchVirtualProvider.prototype, name, {
      configurable: true,
      writable: true,
      value: method
    });
  }
};
__quenchVfsMethods(
  "openSync statSync readdirSync readlinkSync watch watchAsync watchFile unwatchFile",
  function () {
    return function () {
      return this.__notImplemented();
    };
  }
);
__quenchVfsMethods(
  "mkdirSync rmdirSync unlinkSync renameSync linkSync symlinkSync",
  function () {
    return function () {
      return this.__writeCheck();
    };
  }
);
__quenchVfsMethods("stat readdir readlink", function () {
  return async function () {
    return this.__notImplemented();
  };
});
__quenchVfsMethods("copyFileSync writeFileSync appendFileSync", function () {
  return function () {
    return this.__writeCheck();
  };
});
__quenchVfsMethods(
  "mkdir rmdir unlink rename link symlink copyFile writeFile appendFile",
  function () {
    return async function () {
      return this.__writeCheck();
    };
  }
);
class __QuenchMemoryProvider extends __QuenchVirtualProvider {
  constructor() {
    super();
    this._readonly = false;
    const entryPrototype = {
      isFile() {
        return this.type === 0;
      },
      isDirectory() {
        return this.type === 1;
      },
      isSymbolicLink() {
        return this.type === 2;
      },
      isDynamic() {
        return typeof this.contentProvider === "function";
      },
      getContentSync() {
        if (typeof this.contentProvider !== "function") return this.content;
        const value = this.contentProvider();
        if (value && typeof value.then === "function") {
          const error = new Error("Content is async-only");
          error.code = "ERR_INVALID_STATE";
          throw error;
        }
        return value;
      },
      async getContentAsync() {
        return typeof this.contentProvider === "function"
          ? await this.contentProvider()
          : this.content;
      }
    };
    const root = Object.assign(Object.create(entryPrototype), {
      type: 1,
      mode: 0o755,
      children: new Map(),
      populated: true,
      nlink: 1,
      uid: 0,
      gid: 0
    });
    Object.defineProperty(this, Symbol("kRoot"), { value: root });
  }
  get readonly() {
    return this._readonly;
  }
  get supportsSymlinks() {
    return true;
  }
  setReadOnly() {
    this._readonly = true;
  }
}
class __QuenchRealFSProvider extends __QuenchVirtualProvider {
  constructor(root = ".") {
    super();
    if (typeof root !== "string") {
      throw Object.assign(new TypeError("The rootPath argument must be of type string"), { code: "ERR_INVALID_ARG_TYPE" });
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
const __quenchVfsTouch = (entry) => {
  const timestamp = Date.now();
  entry.mtimeMs = timestamp;
  entry.ctimeMs = timestamp;
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
