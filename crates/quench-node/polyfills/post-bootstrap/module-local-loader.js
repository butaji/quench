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
      __nodeFs.readFileSync(path.join(root, "package.json"), "utf8");
      const entry = __quenchPackageEntry(root, subpath);
      return __quenchLocalModulePath(entry, root);
    } catch (_) {}
    const next = path.dirname(directory);
    if (next === directory) break;
    directory = next;
  }
  const error = new Error(`Cannot find module ${specifier}`);
  error.code = "MODULE_NOT_FOUND";
  throw error;
};
const __quenchLocalModulePath = (specifier, parent) => {
  const path = __quenchOriginalRequireWithLocalModules("path");
  const base = specifier.startsWith("/")
    ? specifier
    : path.resolve(path.dirname(parent), specifier);
  for (const candidate of [
    base,
    `${base}.js`,
    `${base}.cjs`,
    `${base}.mjs`,
    `${base}.json`,
    path.join(base, "index.js"),
    path.join(base, "index.cjs"),
    path.join(base, "index.mjs")
  ]) {
    try {
      __nodeFs.readFileSync(candidate, "utf8");
      return candidate;
    } catch (_) {}
  }
  const error = new Error(`Cannot find module ${specifier}`);
  error.code = "MODULE_NOT_FOUND";
  throw error;
};
const __quenchLoadLocalModule = (specifier, parent) => {
  const filename =
    specifier.startsWith(".") || specifier.startsWith("/")
      ? __quenchLocalModulePath(specifier, parent)
      : __quenchPackagePath(specifier, parent);
  if (__quenchLocalModuleCache.has(filename)) {
    return __quenchLocalModuleCache.get(filename).exports;
  }
  const source = __nodeFs.readFileSync(filename, "utf8");
  const module = { exports: {}, children: [], parent: null, filename };
  __quenchLocalModuleCache.set(filename, module);
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
  const path = __quenchOriginalRequireWithLocalModules("path");
  const localRequire = (name) => {
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
  const name = String(specifier);
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
