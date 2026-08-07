const __quenchOriginalRequireWithLocalModules = globalThis.require;
const __quenchLocalModuleCache = new Map();
const __quenchLocalModulePath = (specifier, parent) => {
  const path = __quenchOriginalRequireWithLocalModules("path");
  const base = specifier.startsWith("/")
    ? specifier
    : path.resolve(path.dirname(parent), specifier);
  for (
    const candidate of [
      base,
      `${base}.js`,
      `${base}.json`,
      path.join(base, "index.js"),
    ]
  ) {
    try {
      __nodeFs.readFileSync(candidate, "utf8");
      return candidate;
    } catch (_) {}
  }
  throw new Error(`Cannot find module ${specifier}`);
};
const __quenchLoadLocalModule = (specifier, parent) => {
  const filename = __quenchLocalModulePath(specifier, parent);
  if (__quenchLocalModuleCache.has(filename)) {
    return __quenchLocalModuleCache.get(filename).exports;
  }
  const source = __nodeFs.readFileSync(filename, "utf8");
  const module = { exports: {} };
  __quenchLocalModuleCache.set(filename, module);
  if (filename.endsWith(".json")) {
    module.exports = JSON.parse(source);
    return module.exports;
  }
  const path = __quenchOriginalRequireWithLocalModules("path");
  const localRequire = (name) =>
    name.startsWith(".") || name.startsWith("/")
      ? __quenchLoadLocalModule(name, filename)
      : __quenchOriginalRequireWithLocalModules(name);
  const execute = Function(
    "exports",
    "module",
    "require",
    "__filename",
    "__dirname",
    source,
  );
  execute(
    module.exports,
    module,
    localRequire,
    filename,
    path.dirname(filename),
  );
  return module.exports;
};
globalThis.__quenchLoadLocalModule = (specifier, parent) =>
  __quenchLoadLocalModule(specifier, parent);
globalThis.require = (specifier) => {
  const name = String(specifier);
  if (!name.startsWith(".") && !name.startsWith("/")) {
    return __quenchOriginalRequireWithLocalModules(specifier);
  }
  try {
    return __quenchOriginalRequireWithLocalModules(specifier);
  } catch (_) {}
  return __quenchLoadLocalModule(
    name,
    globalThis.__quench_script_filename || globalThis.__filename,
  );
};
