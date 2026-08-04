const __nodePathFormatExtension = (extension) => {
  if (!extension) return "";
  const value = String(extension);
  return value.startsWith(".") ? value : `.${value}`;
};
const __nodePathFormatParts = (parts) => {
  if (!parts || typeof parts !== "object" || Array.isArray(parts)) {
    const error = new TypeError(
      'The "pathObject" argument must be of type object'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __nodeWindowsRoot = (input) => {
  if (input.startsWith("\\\\")) {
    const parts = input.split("\\");
    return parts.length >= 4 && parts[2] && parts[3]
      ? `\\\\${parts[2]}\\${parts[3]}\\`
      : "\\";
  }
  if (/^[A-Za-z]:\\/.test(input)) return input.slice(0, 3);
  if (/^[A-Za-z]:/.test(input)) return input.slice(0, 2);
  return input.startsWith("\\") ? "\\" : "";
};
const __nodeWindowsParts = (base, root, dir) => {
  const dot = base.lastIndexOf(".");
  const ext = dot > 0 ? base.slice(dot) : "";
  return {
    root,
    dir,
    base,
    ext,
    name: ext ? base.slice(0, -ext.length) : base
  };
};
const __nodeWindowsDir = (trimmed, root, index, trailing) => {
  if (index < 0) return "";
  const keepSeparator =
    trailing ||
    (root.length === 3 && index === 2) ||
    (root.startsWith("\\\\") && index === root.length - 1);
  return trimmed.slice(0, index + (keepSeparator ? 1 : 0)) || root;
};
const __nodeGlobNormalize = (value) =>
  value.replace(/\\/g, "/").replace(/\/+/g, "/").replace(/\/$/, "") || ".";
const __nodeGlobSegment = (value, pattern) => {
  if (pattern.includes("{") && pattern.includes("}")) {
    const open = pattern.indexOf("{");
    const close = pattern.indexOf("}", open);
    const head = pattern.slice(0, open);
    const tail = pattern.slice(close + 1);
    return pattern
      .slice(open + 1, close)
      .split(",")
      .some((part) => __nodeGlobSegment(value, head + part + tail));
  }
  const expression = pattern
    .replace(/[.+^$()|\\]/g, "\\$&")
    .replace(/\{[^}]*\}/g, (match) => match)
    .replace(/\*\*/g, "::DOUBLESTAR::")
    .replace(/\*/g, "[^/]*")
    .replace(/::DOUBLESTAR::/g, ".*")
    .replace(/\?/g, "[^/]")
    .replace(/\[([^\]]+)\]/g, "[$1]");
  try {
    return new RegExp("^" + expression + "$").test(value);
  } catch {
    return false;
  }
};
const __nodeGlobMatch = (path, pattern) => {
  if (typeof path !== "string" || typeof pattern !== "string") return false;
  const paths = __nodeGlobNormalize(path).split("/");
  const patterns = __nodeGlobNormalize(pattern).split("/");
  let pathIndex = 0;
  let patternIndex = 0;
  while (patternIndex < patterns.length) {
    const segment = patterns[patternIndex];
    if (segment === "**") {
      if (patternIndex === patterns.length - 1) return true;
      patternIndex++;
      for (let index = pathIndex; index <= paths.length; index++)
        if (
          __nodeGlobMatch(
            paths.slice(index).join("/"),
            patterns.slice(patternIndex).join("/")
          )
        )
          return true;
      return false;
    }
    if (
      pathIndex >= paths.length ||
      !__nodeGlobSegment(paths[pathIndex], segment)
    )
      return false;
    pathIndex++;
    patternIndex++;
  }
  return pathIndex === paths.length;
};
globalThis.__nodePath = {
  sep: "/",
  delimiter: ":",
  isAbsolute: (value) => __nodePathArg(value).startsWith("/"),
  resolve: (...parts) => {
    let resolved = "";
    let trailing = false;
    for (const part of parts) {
      if (!part) continue;
      resolved = resolved
        ? resolved + "/" + part.replace(/^\/+/, "")
        : part.startsWith("/")
          ? part
          : globalThis.__quench_cwd_get() + "/" + part;
      if (part.endsWith("/") && !part.endsWith("\\")) trailing = true;
    }
    const out = globalThis.__nodePath.normalize(resolved);
    return trailing && !out.endsWith("/") ? out + "/" : out;
  },
  normalize: (value) => {
    const input = __nodePathArg(value);
    const absolute = input.startsWith("/");
    const parts = input.split("/").filter((part) => part && part !== ".");
    const output = [];
    parts.forEach((part) => {
      if (part === ".." && output.length && output[output.length - 1] !== "..")
        output.pop();
      else if (part !== "..") output.push(part);
    });
    const result = (absolute ? "/" : "") + output.join("/");
    return result || (absolute ? "/" : ".");
  },
  basename: (value, suffix) => {
    const base = __nodePathArg(value).replace(/\\/g, "/").split("/").pop();
    if (suffix !== undefined) {
      const end = __nodePathArg(suffix);
      return end && base.endsWith(end) ? base.slice(0, -end.length) : base;
    }
    return base;
  },
  dirname: (value) => {
    const input =
      __nodePathArg(value).replace(/\\/g, "/").replace(/\/+$/, "") || "/";
    const parts = input.split("/");
    parts.pop();
    return parts.join("/") || (input.startsWith("/") ? "/" : ".");
  },
  extname: (value) => {
    const name = globalThis.__nodePath.basename(__nodePathArg(value));
    const i = name.lastIndexOf(".");
    return i > 0 ? name.slice(i) : "";
  },
  join: (...parts) =>
    globalThis.__nodePath.normalize(parts.map(__nodePathArg).join("/")),
  relative: (from, to) => {
    const a = globalThis.__nodePath
      .normalize(__nodePathArg(from))
      .split("/")
      .filter(Boolean);
    const b = globalThis.__nodePath
      .normalize(__nodePathArg(to))
      .split("/")
      .filter(Boolean);
    while (a.length && a[0] === b[0]) {
      a.shift();
      b.shift();
    }
    return [...a.map(() => ".."), ...b].join("/") || "";
  },
  parse: (value) => {
    if (typeof value !== "string") {
      const error = new TypeError('The "path" argument must be of type string');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const input = String(value);
    const trimmed =
      input.replace(/\/+$/, "") || (input.startsWith("/") ? "/" : "");
    const base = globalThis.__nodePath.basename(trimmed);
    const dir = globalThis.__nodePath.dirname(trimmed);
    const ext = globalThis.__nodePath.extname(base);
    return {
      root: input.startsWith("/") ? "/" : "",
      dir,
      base,
      ext,
      name: ext ? base.slice(0, -ext.length) : base
    };
  },
  format: (parts) => {
    __nodePathFormatParts(parts);
    const dir = parts.dir || parts.root || "";
    const extension = __nodePathFormatExtension(parts.ext);
    const base = parts.base || `${parts.name || ""}${extension}`;
    if (!dir) return base;
    if (dir === "/") return `/${base}`;
    return `${dir}/${base}`;
  },
  matchesGlob(path, pattern) {
    return __nodeGlobMatch(path, pattern);
  }
};
globalThis.__nodePath.posix = globalThis.__nodePath;
const __nodeWinPath = {
  sep: "\\",
  delimiter: ";",
  isAbsolute(value) {
    const input = __nodePathArg(value);
    return /^[A-Za-z]:[\\/]/.test(input) || input.startsWith("\\\\");
  },
  normalize(value) {
    const input = __nodePathArg(value).replace(/\//g, "\\");
    const root = /^[A-Za-z]:\\/.test(input) ? input.slice(0, 3) : "";
    const parts = input.slice(root.length).split("\\").filter(Boolean);
    const output = [];
    for (const part of parts) {
      if (part === ".." && output.length && output.at(-1) !== "..")
        output.pop();
      else if (part !== ".") output.push(part);
    }
    return `${root}${output.join("\\")}${output.length ? "" : root ? "" : "."}`;
  },
  join(...parts) {
    return this.normalize(parts.map(__nodePathArg).join("\\"));
  },
  resolve(...parts) {
    return this.normalize(parts.map(__nodePathArg).join("\\"));
  },
  relative(from, to) {
    const a = this.normalize(__nodePathArg(from)).split("\\").filter(Boolean);
    const b = this.normalize(__nodePathArg(to)).split("\\").filter(Boolean);
    while (a.length && a[0].toLowerCase() === b[0].toLowerCase()) {
      a.shift();
      b.shift();
    }
    return [...a.map(() => ".."), ...b].join("\\");
  },
  parse(value) {
    __nodePathArg(value);
    if (value.startsWith("/")) return globalThis.__nodePath.parse(value);
    const input = value.replace(/\//g, "\\");
    const root = __nodeWindowsRoot(input);
    const hadTrailingSeparator = input.length > 0 && /[\\]$/.test(input);
    const trimmed = input.replace(/[\\]+$/, "") || root;
    if (/^[A-Za-z]:$/.test(input))
      return { root: input, dir: "", base: "", ext: "", name: "" };
    if (/^[A-Za-z]:[^\\]/.test(input)) {
      const relative = input.slice(2);
      const dot = relative.lastIndexOf(".");
      const ext = dot > 0 ? relative.slice(dot) : "";
      return __nodeWindowsParts(relative, input.slice(0, 2), "");
    }
    if (root.startsWith("\\\\") && trimmed === root.slice(0, -1))
      return { root, dir: root, base: "", ext: "", name: "" };
    const index = trimmed.lastIndexOf("\\");
    const dir = __nodeWindowsDir(trimmed, root, index, hadTrailingSeparator);
    const base = index >= 0 ? trimmed.slice(index + 1) : trimmed;
    return __nodeWindowsParts(base, root, dir);
  },
  format(parts) {
    __nodePathFormatParts(parts);
    const extension = __nodePathFormatExtension(parts.ext);
    const base = parts.base || `${parts.name || ""}${extension}`;
    const dir = parts.dir || parts.root || "";
    if (!dir) return base;
    if (/^[A-Za-z]:$/.test(dir)) return `${dir}${base}`;
    if (dir.endsWith("\\")) return `${dir}${base}`;
    return `${dir}\\${base}`;
  },
  basename: (value, suffix) => {
    const base =
      __nodePathArg(value)
        .replace(/[\\/]+$/, "")
        .split(/[\\/]/)
        .pop() || "";
    if (suffix !== undefined) {
      const end = __nodePathArg(suffix);
      return end && base.endsWith(end) ? base.slice(0, -end.length) : base;
    }
    return base;
  },
  dirname: (value) => {
    const input = __nodePathArg(value).replace(/[\\/]+$/, "");
    const index = input.lastIndexOf("\\");
    return index < 0 ? "" : input.slice(0, index) || "\\";
  },
  extname(value) {
    const base = this.basename(__nodePathArg(value));
    const index = base.lastIndexOf(".");
    return index > 0 ? base.slice(index) : "";
  }
};
__nodeWinPath.posix = globalThis.__nodePath;
__nodeWinPath.win32 = __nodeWinPath;
globalThis.__nodePath.win32 = __nodeWinPath;

globalThis.__nodeCommon = {
  mustCall: (fn, exact = 1) => {
    let calls = 0;
    const wrapped = function (...args) {
      calls++;
      wrapped.calls = calls;
      return fn(...args);
    };
    wrapped.calls = 0;
    wrapped.expected = exact;
    wrapped.__quench_index = (globalThis.__nodeCallChecks ||= []).length;
    globalThis.__nodeCallChecks.push(wrapped);
    return wrapped;
  },
  mustCallAtLeast: (fn, minimum = 1) => {
    const wrapped = globalThis.__nodeCommon.mustCall(fn, minimum);
    wrapped.__quench_at_least = true;
    return wrapped;
  },
  mustSucceed: (fn = () => {}) =>
    globalThis.__nodeCommon.mustCall((error, ...args) => {
      if (error) throw error;
      return fn(...args);
    }),
  mustNotCall:
    (message = "Unexpected call") =>
    () => {
      throw new Error(message);
    },
  noop: () => {},
  isAlive: (pid) => {
    const alive = globalThis.__quench_node_pids || new Set();
    globalThis.__quench_node_pids = alive;
    return alive.has(pid);
  },
  printSkipMessage: (message) => console.log(`# SKIP: ${message}`),
  expectsError: (_expected) => (error) => {
    if (!error) throw new Error("Expected filesystem error");
  },
  invalidArgTypeHelper: (input) => {
    if (input == null) return ` Received ${input}`;
    let rendered;
    try {
      rendered = String(input);
    } catch (_) {
      rendered = Object.prototype.toString.call(input);
    }
    return ` Received type ${typeof input} (${rendered})`;
  },
  expectWarning: (_type, _message) => {},
  mustNotMutateObjectDeep: (value) => value,
  isLinux: process.platform === "linux",
  hasIntl: typeof Intl !== "undefined",
  isDebug: false,
  isMacOS: process.platform === "darwin",
  isWindows: process.platform === "win32",
  isAIX: false,
  isFreeBSD: false,
  enoughTestMem: true,
  canCreateSymLink: () => process.platform !== "win32",
  getArrayBufferViews: (buffer) => [
    buffer,
    new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength),
    new DataView(buffer.buffer, buffer.byteOffset, buffer.byteLength)
  ]
};
globalThis.__quench_verify_calls = () => {
  for (const callback of globalThis.__nodeCallChecks || []) {
    if (
      callback.__quench_at_least
        ? callback.calls < callback.expected
        : callback.calls !== callback.expected
    )
      throw new Error(
        `Callback ${callback.__quench_index}: expected ${callback.expected} calls, got ${callback.calls}`
      );
  }
};
globalThis.__nodeTmpdir = {
  path: `/tmp/quench-node-${process.pid}`,
  hasEnoughSpace: (_bytes) => false,
  refresh: () => {
    try {
      globalThis.__quench_fs_mkdir(globalThis.__nodeTmpdir.path);
    } catch (_) {}
  },
  resolve: (name = "") =>
    globalThis.__nodePath.join(globalThis.__nodeTmpdir.path, String(name)),
  fileURL: (name = "") =>
    new globalThis.__nodeURL(
      `file://${globalThis.__nodePath.join(globalThis.__nodeTmpdir.path, String(name))}`
    )
};
class NodeEventEmitter {
  constructor(options = {}) {
    this._events = {};
    this.captureRejections =
      options.captureRejections ?? NodeEventEmitter.captureRejections ?? false;
  }
  on(event, listener) {
    (this._events[event] ||= []).push(listener);
    return this;
  }
  addListener(event, listener) {
    return this.on(event, listener);
  }
  once(event, listener) {
    const wrapped = (...args) => {
      this.removeListener(event, wrapped);
      listener(...args);
    };
    return this.on(event, wrapped);
  }
  emit(event, ...args) {
    if (event === "error") {
      const monitorSymbol =
        globalThis.__nodeErrorMonitorSymbol ||
        Symbol.for("events.errorMonitor");
      const monitor = this._events[monitorSymbol] || [];
      monitor.slice().forEach((listener) => listener(...args));
    }
    const listeners = this._events[event] || [];
    listeners.slice().forEach((listener) => {
      const result = listener(...args);
      if (this.captureRejections && result?.then)
        result.catch((error) =>
          queueMicrotask(() => {
            const rejection = this[Symbol.for("nodejs.rejection")];
            if (typeof rejection === "function")
              rejection.call(this, error, event, ...args);
            else this.emit("error", error);
          })
        );
    });
    return listeners.length > 0;
  }
  removeListener(event, listener) {
    this._events[event] = (this._events[event] || []).filter(
      (item) => item !== listener
    );
    return this;
  }
  off(event, listener) {
    return this.removeListener(event, listener);
  }
  removeAllListeners(event) {
    if (event === undefined) this._events = {};
    else delete this._events[event];
    return this;
  }
  listeners(event) {
    return (this._events[event] || []).slice();
  }
  listenerCount(event) {
    return (this._events[event] || []).length;
  }
}
globalThis.__nodeEventEmitter = NodeEventEmitter;
globalThis.process._events = {};
