{
  if (globalThis.require) {
    const moduleApi = globalThis.require("module");
    const builtins = new Set(
      "assert buffer child_process cluster console crypto dgram diagnostics_channel dns events fs http https module net os path perf_hooks process punycode querystring readline repl stream string_decoder sys timers tls trace_events tty url util v8 vm wasi worker_threads zlib".split(
        " "
      )
    );
    moduleApi.builtinModules ||= [];
    for (const name of builtins) {
      if (!moduleApi.builtinModules.includes(name)) {
        moduleApi.builtinModules.push(name);
      }
    }
    moduleApi.isBuiltin ||= (name) =>
      builtins.has(String(name).replace(/^node:/, ""));
    moduleApi.createRequire ||= () => globalThis.require;
    moduleApi.findSourceMap ||= () => undefined;
    moduleApi.syncBuiltinESMExports ||= () => undefined;
    moduleApi.register ||= () => undefined;
    moduleApi.registerHooks ||= () => ({});
    moduleApi.runMain ||= () => undefined;
    moduleApi.findPackageJSON ||= () => undefined;
    moduleApi.getSourceMapsSupport ||= () => ({});
    moduleApi.setSourceMapsSupport ||= () => undefined;
    moduleApi.stripTypeScriptTypes ||= (source) => String(source);
    moduleApi.enableCompileCache ||= () => ({});
    moduleApi.flushCompileCache ||= () => undefined;
    moduleApi.getCompileCacheDir ||= () => undefined;
    moduleApi.constants ||= {
      compileCacheStatus: {
        FAILED: 0,
        ENABLED: 1,
        ALREADY_ENABLED: 2,
        DISABLED: 3
      }
    };
    moduleApi.SourceMap ||= function SourceMap() {};
    if (typeof moduleApi.Module !== "function") {
      moduleApi.Module = function Module() {};
    }
    moduleApi.Module.isBuiltin ||= moduleApi.isBuiltin;
    moduleApi.Module.createRequire ||= moduleApi.createRequire;
    moduleApi.Module.builtinModules ||= moduleApi.builtinModules;
    moduleApi.Module._cache ||= {};
    moduleApi.Module._extensions ||= {};
    for (const extension of [".js", ".json", ".node"]) {
      moduleApi.Module._extensions[extension] ||= () => undefined;
    }
    moduleApi.Module.globalPaths ||= moduleApi.globalPaths || [];
    moduleApi.Module._pathCache ||= {};
    moduleApi.Module._nodeModulePaths ||= () => [];
    moduleApi.Module._findPath ||= () => false;
    moduleApi.Module._resolveLookupPaths ||= () => [];
    moduleApi.Module._load ||= () => undefined;
  }
}
