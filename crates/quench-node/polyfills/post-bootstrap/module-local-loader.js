const __quenchOriginalRequireWithLocalModules = globalThis.require;
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
  const parts = specifier.startsWith("@")
    ? specifier.split("/").slice(0, 2)
    : specifier.split("/").slice(0, 1);
  const packageName = parts.join("/");
  const subpath = specifier.slice(packageName.length).replace(/^\//, "");
  let directory = path.dirname(parent);
  while (true) {
    const root = path.join(directory, "node_modules", packageName);
    try {
      const entry = __quenchPackageEntry(root, subpath);
      return __quenchLocalModulePath(entry, root);
    } catch (_) {}
    const next = path.dirname(directory);
    if (next === directory) break;
    directory = next;
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
  try {
    filename = __nodeFs.realpathSync(filename);
  } catch (_) {}
  if (
    __quenchLocalModuleCache.has(filename) &&
    globalThis.require.cache &&
    !globalThis.require.cache[filename]
  ) {
    __quenchLocalModuleCache.delete(filename);
  }
  if (__quenchLocalModuleCache.has(filename)) {
    return __quenchLocalModuleCache.get(filename).exports;
  }
  if (filename.endsWith(".node")) {
    const error = new Error(`file too short: ${filename}`);
    error.code = "ERR_DLOPEN_FAILED";
    throw error;
  }
  const source = __nodeFs.readFileSync(filename, "utf8");
  const path = __quenchOriginalRequireWithLocalModules("path");
  const module = { exports: {}, children: [], parent: null, filename };
  __quenchLocalModuleCache.set(filename, module);
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
      return childExports;
    }
    try {
      return __quenchOriginalRequireWithLocalModules(name);
    } catch (_) {
      return __quenchLoadLocalModule(name, filename);
    }
  };
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
  return module.exports;
};
globalThis.__quenchLoadLocalModule = (specifier, parent) =>
  __quenchLoadLocalModule(specifier, parent);
globalThis.require = (specifier) => {
  __quenchValidateRequireId(specifier);
  const name = specifier;
  if (!name.startsWith(".") && !name.startsWith("/")) {
    try {
      return __quenchOriginalRequireWithLocalModules(specifier);
    } catch (_) {
      return __quenchLoadLocalModule(
        name,
        globalThis.__quench_script_filename || globalThis.__filename
      );
    }
  }
  try {
    return __quenchOriginalRequireWithLocalModules(specifier);
  } catch (_) {}
  return __quenchLoadLocalModule(
    name,
    globalThis.__quench_script_filename || globalThis.__filename
  );
};
globalThis.module ||= {
  exports: {},
  children: [],
  parent: null,
  filename: globalThis.__quench_script_filename || globalThis.__filename
};
globalThis.require.cache ||= Object.create(null);
globalThis.require.extensions =
  __quenchOriginalRequireWithLocalModules("module")._extensions;
