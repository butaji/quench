const __quenchOriginalRequireWithModule = globalThis.require;
const __quenchBuiltinModules = [
  "assert",
  "buffer",
  "child_process",
  "cluster",
  "crypto",
  "events",
  "fs",
  "http",
  "https",
  "module",
  "net",
  "os",
  "path",
  "perf_hooks",
  "process",
  "querystring",
  "stream",
  "string_decoder",
  "timers",
  "tls",
  "tty",
  "url",
  "util",
  "vm",
  "worker_threads",
  "zlib"
];
const formatValue = (value) => {
  if (value && typeof value === "object") {
    try {
      return JSON.stringify(value);
    } catch (_) {
      return Object.prototype.toString.call(value);
    }
  }
  return String(value);
};
const __quenchModule = {
  builtinModules: __quenchBuiltinModules,
  _cache: Object.create(null),
  _extensions: Object.create(null),
  createRequire: (filename) => {
    const pathApi = __quenchOriginalRequireWithModule("path");
    const isFileUrl =
      typeof filename === "string" && filename.startsWith("file://");
    const isFileUrlObject =
      typeof URL === "function" &&
      filename instanceof URL &&
      filename.protocol === "file:";
    const raw = isFileUrlObject ? filename.pathname : String(filename || "");
    if (typeof filename !== "string" && !isFileUrlObject) {
      const error = new TypeError(
        `The argument 'filename' must be a file URL object, file URL string, or absolute path string. Received ${formatValue(filename)}`
      );
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    if (!isFileUrl && !isFileUrlObject && !pathApi.isAbsolute(raw)) {
      const error = new TypeError(
        "The argument 'filename' must be a file URL object, file URL string, or absolute path string"
      );
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    const base = isFileUrl ? raw.slice(7) : raw;
    const directory = base ? pathApi.dirname(base) : process.cwd();
    return (specifier) => {
      const value = String(specifier);
      if (value.startsWith(".")) {
        return __quenchOriginalRequireWithModule(
          pathApi.resolve(directory, value)
        );
      }
      return __quenchOriginalRequireWithModule(specifier);
    };
  },
  isBuiltin: (name) =>
    __quenchBuiltinModules.includes(String(name).replace(/^node:/, "")),
  _resolveLookupPaths: (request) => {
    const value = String(request);
    if (/^\.\.?\//.test(value) || value.startsWith("/")) return ["."];
    return ["node_modules"];
  }
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "module") {
    return __quenchModule;
  }
  return __quenchOriginalRequireWithModule(specifier);
};
