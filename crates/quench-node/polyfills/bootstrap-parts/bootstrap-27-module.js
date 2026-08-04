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
const __quenchModule = {
  builtinModules: __quenchBuiltinModules,
  _cache: Object.create(null),
  _extensions: Object.create(null),
  createRequire: () => __quenchOriginalRequireWithModule,
  isBuiltin: (name) =>
    __quenchBuiltinModules.includes(String(name).replace(/^node:/, ""))
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "module")
    return __quenchModule;
  return __quenchOriginalRequireWithModule(specifier);
};
