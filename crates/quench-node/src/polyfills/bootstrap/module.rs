//! Polyfill: `module`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithModule = globalThis.require;
const __quenchBuiltinModules =
  "assert assert/strict async_hooks buffer child_process cluster console crypto diagnostics_channel dns dns/promises events fs fs/promises http http2 https module net os path perf_hooks process punycode querystring readline readline/promises repl stream stream/consumers stream/promises stream/web string_decoder sys timers timers/promises tls trace_events tty url util v8 vm wasi worker_threads zlib".split(
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
  const isFileUrlString =
    typeof filename === "string" && filename.startsWith("file:");
  const isFileUrlObject =
    typeof URL === "function" &&
    filename instanceof URL &&
    filename.protocol === "file:";
  let isFileUrl = isFileUrlString || isFileUrlObject;
  let raw;
  if (isFileUrlObject) {
    raw = decodeFilePath(filename.pathname);
  } else if (isFileUrlString) {
    try {
      const parsed = new URL(filename);
      if (parsed.protocol !== "file:") isFileUrl = false;
      raw = isFileUrl ? decodeFilePath(parsed.pathname) : "";
    } catch (_) {
      raw = "";
      isFileUrl = false;
    }
  } else {
    raw = String(filename || "");
  }
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
const __quenchFindPackageJson = (specifier, base) => {
  if (typeof specifier !== "string") {
    const error = new TypeError("The \"specifier\" argument must be of type string");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const pathApi = __quenchOriginalRequireWithModule("path");
  const fsApi = __quenchOriginalRequireWithModule("fs");
  const start = pathApi.resolve(base === undefined ? process.cwd() : String(base));
  const isPath = specifier.startsWith("./") || specifier.startsWith("../") ||
    specifier.startsWith("/") || specifier.startsWith("file:");
  let candidate;
  if (isPath) {
    candidate = specifier.startsWith("file:")
      ? decodeFilePath(new URL(specifier).pathname)
      : pathApi.resolve(start, specifier);
  } else {
    const parts = specifier.split("/");
    const packageName = specifier.startsWith("@") ? parts.slice(0, 2).join("/") : parts[0];
    let current = start;
    while (true) {
      const packageRoot = pathApi.join(current, "node_modules", packageName);
      try {
        if (fsApi.statSync(packageRoot).isDirectory()) {
          candidate = packageRoot;
          break;
        }
      } catch (_) {}
      const parent = pathApi.dirname(current);
      if (parent === current) break;
      current = parent;
    }
    if (!candidate) return undefined;
  }
  try {
    if (!fsApi.statSync(candidate).isDirectory()) candidate = pathApi.dirname(candidate);
  } catch (_) {}
  while (true) {
    const packageJson = pathApi.join(candidate, "package.json");
    try {
      if (fsApi.statSync(packageJson).isFile()) return packageJson;
    } catch (_) {}
    const parent = pathApi.dirname(candidate);
    if (parent === candidate) return undefined;
    candidate = parent;
  }
};
const __quenchModule = {
  builtinModules: __quenchBuiltinModules,
  _cache: Object.create(null),
  _extensions: Object.create(null),
  createRequire: (filename) => {
    const pathApi = __quenchOriginalRequireWithModule("path");
    const { raw } = __quenchRequireFilename(filename, pathApi);
    const base = raw;
    const directory = base ? pathApi.dirname(base) : process.cwd();
    const created = __quenchCreatedRequire(directory, pathApi);
    created.resolve = (specifier) => {
      const value = String(specifier);
      if (!value.startsWith(".") && !value.startsWith("/")) return value;
      return pathApi.resolve(directory, value);
    };
    return created;
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
  findPackageJSON: __quenchFindPackageJson,
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
"#);
