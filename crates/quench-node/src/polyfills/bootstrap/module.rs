//! Polyfill: `module`

pub const JS: &str = r#"const __quenchOriginalRequireWithModule = globalThis.require;
const __quenchBuiltinModules =
  "assert buffer child_process cluster crypto events fs http https module net os path perf_hooks process querystring stream string_decoder timers tls tty url util vm worker_threads zlib".split(
    " "
  );
const decodeFilePath = (value) => {
  try {
    return decodeURIComponent(value);
  } catch (_) {
    return value;
  }
};
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
const nodeModulePaths = (from) => {
  const pathApi = __quenchOriginalRequireWithModule("path");
  const value = String(from);
  const result = [];
  let current = pathApi.resolve(value);
  while (true) {
    const candidate =
      pathApi.basename(current) === "node_modules"
        ? current
        : pathApi.join(current, "node_modules");
    if (result[result.length - 1] !== candidate) result.push(candidate);
    const parent = pathApi.dirname(current);
    if (parent === current) break;
    current = parent;
  }
  return result;
};
const moduleStat = (filename) => {
  try {
    const stat = __quenchOriginalRequireWithModule("fs").statSync(filename);
    return stat.isDirectory() ? 1 : 0;
  } catch (_) {
    return -1;
  }
};
const setSourceMapsSupport = (enabled, options) => {
  if (typeof enabled !== "boolean") {
    const error = new TypeError(
      "enabled must be a boolean (ERR_INVALID_ARG_TYPE)"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options !== undefined && (!options || typeof options !== "object")) {
    const error = new TypeError(
      "options must be an object (ERR_INVALID_ARG_TYPE)"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  for (const name of ["nodeModules", "generatedCode"]) {
    if (options?.[name] !== undefined && typeof options[name] !== "boolean") {
      const error = new TypeError(
        `${name} must be a boolean (ERR_INVALID_ARG_TYPE)`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  }
};
const globalPaths = [];
const initPaths = () => {
  globalPaths.length = 0;
  const delimiter = process.platform === "win32" ? ";" : ":";
  for (const entry of String(process.env.NODE_PATH || "").split(delimiter)) {
    if (entry) globalPaths.push(entry);
  }
  return globalPaths;
};
const __quenchValidateRequireFilename = (
  filename,
  pathApi,
  isFileUrl,
  isFileUrlObject,
  raw
) => {
  if (typeof filename !== "string" && !isFileUrlObject) {
    const error = new TypeError(
      `The argument 'filename' must be a file URL object, file URL string, or absolute path string. Received ${formatValue(
        filename
      )}`
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  if (!isFileUrl && !isFileUrlObject && !pathApi.isAbsolute(raw)) {
    throw Object.assign(new TypeError("The argument 'filename' must be a file URL object, file URL string, or absolute path string"), { code: "ERR_INVALID_ARG_VALUE" });
  }
};
const __quenchRequireFilename = (filename, pathApi) => {
  const isFileUrl =
    typeof filename === "string" && filename.startsWith("file://");
  const isFileUrlObject =
    typeof URL === "function" &&
    filename instanceof URL &&
    filename.protocol === "file:";
  const raw = isFileUrlObject
    ? decodeFilePath(filename.pathname)
    : String(filename || "");
  __quenchValidateRequireFilename(
    filename,
    pathApi,
    isFileUrl,
    isFileUrlObject,
    raw
  );
  return { isFileUrl, raw, isFileUrlObject };
};
const __quenchCreatedRequire = (directory, pathApi) => (specifier) => {
  const value = String(specifier);
  return value.startsWith(".")
    ? __quenchOriginalRequireWithModule(pathApi.resolve(directory, value))
    : __quenchOriginalRequireWithModule(specifier);
};
const __quenchModule = {
  builtinModules: __quenchBuiltinModules,
  _cache: Object.create(null),
  _extensions: Object.create(null),
  createRequire: (filename) => {
    const pathApi = __quenchOriginalRequireWithModule("path");
    const { isFileUrl, raw } = __quenchRequireFilename(filename, pathApi);
    const base = isFileUrl ? decodeFilePath(raw.slice(7)) : raw;
    const directory = base ? pathApi.dirname(base) : process.cwd();
    return __quenchCreatedRequire(directory, pathApi);
  },
  isBuiltin: (name) =>
    __quenchBuiltinModules.includes(String(name).replace(/^node:/, "")),
  _resolveLookupPaths: (request) => {
    const value = String(request);
    if (/^\.\.?\//.test(value) || value.startsWith("/")) return ["."];
    return ["node_modules"];
  },
  _nodeModulePaths: nodeModulePaths,
  _stat: moduleStat,
  setSourceMapsSupport,
  globalPaths,
  _initPaths: initPaths
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "module") {
    return __quenchModule;
  }
  return __quenchOriginalRequireWithModule(specifier);
};
"#;
