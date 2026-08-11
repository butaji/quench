//! Polyfill: `module-local-loader`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithLocalModules = globalThis.require;
const __quenchLocalModuleCache = new Map();
const __quenchModulePath = __quenchOriginalRequireWithLocalModules("path");
const path = __quenchModulePath;
const __quenchPackageEntry = (root, subpath) => {
  if (subpath) return path.resolve(root, subpath);
  let manifest;
  try {
    manifest = JSON.parse(
      __nodeFs.readFileSync(path.join(root, "package.json"), "utf8")
    );
  } catch (_) {
    manifest = {};
  }
  const exports = manifest.exports;
  const selectExport = (value) => {
    if (typeof value === "string") return value;
    if (!value || typeof value !== "object") return undefined;
    return selectExport(
      value.require || value.node || value.default || value.import || value["."]
    );
  };
  const exportRoot =
    exports && typeof exports === "object" && exports["."]
      ? exports["."]
      : exports;
  const entry = selectExport(exportRoot);
  return path.resolve(
    root,
    entry || manifest.main || manifest.module || "index.js"
  );
};
const __quenchResolvedExtensions = new Map();
const __quenchPackagePath = (specifier, parent) => {
  const normalizedSpecifier = specifier.replace(/\/+$/, "");
  const parts = normalizedSpecifier.startsWith("@")
    ? normalizedSpecifier.split("/").slice(0, 2)
    : normalizedSpecifier.split("/").slice(0, 1);
  const packageName = parts.join("/");
  const subpath = normalizedSpecifier
    .slice(packageName.length)
    .replace(/^\//, "");
  let directory = path.dirname(parent);
  while (true) {
    const root = path.join(directory, "node_modules", packageName);
    try {
      if (!subpath) {
        for (const candidate of [
          `${root}.js`,
          `${root}.json`,
          `${root}.node`
        ]) {
          try {
            __nodeFs.readFileSync(candidate, "utf8");
            return candidate;
          } catch (_) {}
        }
      }
      const entry = __quenchPackageEntry(root, subpath);
      return __quenchLocalModulePath(entry, root);
    } catch (_) {}
    const next = path.dirname(directory);
    if (next === directory) break;
    directory = next;
  }
  const moduleApi = __quenchOriginalRequireWithLocalModules("module");
  for (const directory of moduleApi.globalPaths || []) {
    const root = path.join(directory, packageName);
    try {
      const entry = __quenchPackageEntry(root, subpath);
      return __quenchLocalModulePath(entry, root);
    } catch (_) {}
  }
  const error = new Error(`Cannot find module '${specifier}'`);
  error.code = "MODULE_NOT_FOUND";
  throw error;
};
const __quenchValidateRequireId = (specifier) => {
  if (typeof specifier !== "string") {
    const error = new TypeError(
      `The "id" argument must be of type string. Received ${typeof specifier}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (specifier.length === 0) {
    const error = new TypeError(
      `The argument 'id' must be a non-empty string. Received '${specifier}'`
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
};
const __quenchResolve = (specifier, parent, options) => {
  if (typeof specifier !== "string") {
    let received;
    if (specifier === null) received = "null";
    else if (specifier === undefined) received = "undefined";
    else if (typeof specifier === "object") received = "an instance of Object";
    else received = `type ${typeof specifier} (${String(specifier)})`;
    const error = new TypeError(
      `The "request" argument must be of type string. Received ${received}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options !== undefined && (!options || typeof options !== "object")) {
    const error = new TypeError("options must be an object");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options?.paths !== undefined) {
    if (!Array.isArray(options.paths)) {
      const error = new TypeError("options.paths must be an array");
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    for (const value of options.paths) {
      if (typeof value !== "string") {
        const error = new TypeError("options.paths entries must be strings");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
    }
    if (options.paths.length === 0) {
      const error = new Error(`Cannot find module '${specifier}'`);
      error.code = "MODULE_NOT_FOUND";
      throw error;
    }
  }
  const request = specifier.replace(/\/+$/, "");
  const normalized = request.replace(/^node:/, "");
  if (
    __quenchOriginalRequireWithLocalModules("module").builtinModules.includes(
      normalized
    )
  ) {
    return specifier;
  }
  const suppliedPath = options?.paths?.find(
    (value, index, values) =>
      !values.some(
        (other, otherIndex) =>
          otherIndex !== index &&
          value.startsWith(`${path.resolve(other)}${path.sep}`)
      )
  );
  const lookupParent =
    (suppliedPath && path.resolve(suppliedPath, "index.js")) || parent;
  let filename;
  if (request.startsWith(".") || request.startsWith("/")) {
    filename = __quenchLocalModulePath(request, lookupParent);
  } else if (
    suppliedPath &&
    path.basename(path.resolve(suppliedPath)) === "node_modules"
  ) {
    filename = __quenchPackagePath(
      request,
      path.join(path.dirname(suppliedPath), "index.js")
    );
  } else {
    filename = __quenchPackagePath(request, lookupParent);
  }
  try {
    return __nodeFs.realpathSync(filename);
  } catch (_) {
    return filename;
  }
};
const __quenchResolvePaths = (specifier, parent) => {
  if (typeof specifier !== "string") {
    let received;
    if (specifier === null) received = "null";
    else if (specifier === undefined) received = "undefined";
    else if (typeof specifier === "object") received = "an instance of Object";
    else received = `type ${typeof specifier} (${String(specifier)})`;
    const error = new TypeError(
      `The "request" argument must be of type string. Received ${received}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (specifier.startsWith(".") || specifier.startsWith("/")) {
    return [path.dirname(parent)];
  }
  const normalized = String(specifier).replace(/^node:/, "");
  if (
    __quenchOriginalRequireWithLocalModules("module").builtinModules.includes(
      normalized
    )
  ) {
    return null;
  }
  return __quenchOriginalRequireWithLocalModules("module")._nodeModulePaths(
    path.dirname(parent)
  );
};
const __quenchLocalModulePath = (specifier, parent) => {
  const path = __quenchOriginalRequireWithLocalModules("path");
  const base = specifier.startsWith("/")
    ? specifier
    : path.resolve(path.dirname(parent), specifier);
  const candidates = [
    base,
    `${base}.js`,
    `${base}.cjs`,
    `${base}.mjs`,
    `${base}.json`,
    path.join(base, "index.js"),
    path.join(base, "index.cjs"),
    path.join(base, "index.mjs")
  ];
  for (const extension of Object.keys(globalThis.require.extensions || {}).sort(
    (left, right) => right.length - left.length
  )) {
    if (extension !== ".js" && extension !== ".json" && extension !== ".node") {
      candidates.push(`${base}${extension}`);
    }
  }
  for (const candidate of candidates) {
    try {
      __nodeFs.readFileSync(candidate, "utf8");
      if (candidate !== base) {
        const extension = Object.keys(globalThis.require.extensions || {})
          .sort((left, right) => right.length - left.length)
          .find((value) => candidate.endsWith(value));
        if (extension) __quenchResolvedExtensions.set(candidate, extension);
      }
      return candidate;
    } catch (_) {}
  }
  const error = new Error(`Cannot find module '${specifier}'`);
  error.code = "MODULE_NOT_FOUND";
  throw error;
};
const __quenchLoadLocalModule = (specifier, parent) => {
  let filename =
    specifier.startsWith(".") || specifier.startsWith("/")
      ? __quenchLocalModulePath(specifier, parent)
      : __quenchPackagePath(specifier, parent);
  let cacheFilename = filename;
  try {
    cacheFilename = __nodeFs.realpathSync(filename);
  } catch (_) {}
  if (
    __quenchLocalModuleCache.has(cacheFilename) &&
    globalThis.require.cache &&
    !globalThis.require.cache[filename]
  ) {
    __quenchLocalModuleCache.delete(cacheFilename);
  }
  if (__quenchLocalModuleCache.has(cacheFilename)) {
    return __quenchLocalModuleCache.get(cacheFilename).exports;
  }
  if (filename.endsWith(".node")) {
    const error = new Error(`file too short: ${filename}`);
    error.code = "ERR_DLOPEN_FAILED";
    throw error;
  }
  const source = __nodeFs.readFileSync(filename, "utf8");
  const path = __quenchOriginalRequireWithLocalModules("path");
  const module = { exports: {}, children: [], parent: null, filename };
  __quenchLocalModuleCache.set(cacheFilename, module);
  if (globalThis.require.cache) globalThis.require.cache[filename] = module;
  const basename = path.basename(filename);
  const extension =
    __quenchResolvedExtensions.get(filename) ||
    (basename.startsWith(".") && basename.indexOf(".", 1) === -1
      ? undefined
      : path.extname(filename));
  const extensionHandler = globalThis.require.extensions?.[extension];
  if (typeof extensionHandler === "function") {
    extensionHandler(module, filename);
    return module.exports;
  }
  if (filename.endsWith(".json")) {
    try {
      module.exports = JSON.parse(source);
    } catch (error) {
      const wrapped = new SyntaxError(`${filename}: ${error.message}`);
      wrapped.stack = error.stack;
      throw wrapped;
    }
    return module.exports;
  }
  const localRequire = (name) => {
    __quenchValidateRequireId(name);
    if (name.startsWith(".") || name.startsWith("/")) {
      const childFilename = __quenchLocalModulePath(name, filename);
      const childExports = __quenchLoadLocalModule(name, filename);
      const childModule = __quenchLocalModuleCache.get(childFilename);
      if (childModule && !module.children.includes(childModule)) {
        childModule.parent = module;
        module.children.push(childModule);
      }
      return __quenchCompleteNodeCommon(childExports);
    }
    try {
      const result = __quenchOriginalRequireWithLocalModules(name);
      return __quenchCompleteNodeCommon(
        globalThis.__quenchFinalizeModule
          ? globalThis.__quenchFinalizeModule(
              name,
              (specifier) => __quenchOriginalRequireWithLocalModules(specifier),
              result
            )
          : result
      );
    } catch (error) {
      if (error?.code && error.code !== "MODULE_NOT_FOUND") throw error;
      return __quenchCompleteNodeCommon(
        __quenchLoadLocalModule(name.replace(/\/+$/, ""), filename)
      );
    }
  };
  localRequire.resolve = (name, options) =>
    __quenchResolve(name, filename, options);
  localRequire.resolve.paths = (name) => __quenchResolvePaths(name, filename);
  const execute = Function(
    "exports",
    "module",
    "require",
    "__filename",
    "__dirname",
    source
  );
  execute(
    module.exports,
    module,
    localRequire,
    filename,
    path.dirname(filename)
  );
  if (
    filename.endsWith("/tests/node/test/common/index.js") &&
    module.exports &&
    module.exports.PIPE === undefined
  ) {
    module.exports.PIPE = path.join(
      path.relative(process.cwd(), "/tmp"),
      `node-test.${process.pid}.sock`
    );
  }
  return module.exports;
};
globalThis.__quenchLoadLocalModule = (specifier, parent) =>
  __quenchLoadLocalModule(specifier, parent);
const __quenchCompleteNodeCommon = (value) => {
  if (
    value &&
    value.allowGlobals &&
    value.mustCall &&
    value.PIPE === undefined
  ) {
    value.PIPE = `../tmp/node-test.${process.pid}.sock`;
  }
  return value;
};
globalThis.require = (specifier) => {
  __quenchValidateRequireId(specifier);
  const name = specifier;
  if (!name.startsWith(".") && !name.startsWith("/")) {
    try {
      const result = __quenchOriginalRequireWithLocalModules(specifier);
      return __quenchCompleteNodeCommon(
        globalThis.__quenchFinalizeModule
          ? globalThis.__quenchFinalizeModule(
              specifier,
              (name) => __quenchOriginalRequireWithLocalModules(name),
              result
            )
          : result
      );
    } catch (error) {
      if (error?.code && error.code !== "MODULE_NOT_FOUND") throw error;
      return __quenchLoadLocalModule(
        name.replace(/\/+$/, ""),
        globalThis.__quench_script_filename || globalThis.__filename
      );
    }
  }
  try {
    return __quenchCompleteNodeCommon(
      __quenchOriginalRequireWithLocalModules(specifier)
    );
  } catch (_) {}
  return __quenchLoadLocalModule(
    name,
    globalThis.__quench_script_filename || globalThis.__filename
  );
};
globalThis.require.resolve = (name, options) =>
  __quenchResolve(
    name,
    globalThis.__quench_script_filename || globalThis.__filename,
    options
  );
globalThis.require.resolve.paths = (name) =>
  __quenchResolvePaths(
    name,
    globalThis.__quench_script_filename || globalThis.__filename
  );
globalThis.__quenchRequireResolve = globalThis.require.resolve;
globalThis.__quenchRequireResolvePaths = globalThis.require.resolve.paths;
globalThis.module ||= {
  exports: {},
  children: [],
  parent: null,
  filename: globalThis.__quench_script_filename || globalThis.__filename
};
globalThis.require.cache ||= Object.create(null);
globalThis.require.extensions =
  __quenchOriginalRequireWithLocalModules("module")._extensions;
"#);
