//! Polyfill: `module-surface-00`

pub const JS: &str = quench_js_check::checked_js!(r#"{
  if (globalThis.require) {
    const moduleApi = globalThis.require("module");
    const builtins = new Set(
      "assert assert/strict async_hooks buffer child_process cluster console constants crypto dgram diagnostics_channel dns dns/promises domain events fs fs/promises http http2 https inspector inspector/promises module net os path path/posix path/win32 perf_hooks process punycode querystring readline readline/promises repl sea stream stream/consumers stream/promises stream/web string_decoder sys test test/reporters timers timers/promises tls trace_events tty url util util/types v8 vm wasi worker_threads zlib sqlite".split(
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
    moduleApi.createRequire = (filename) => {
      const path = globalThis.require("path");
      const value = filename instanceof URL ? filename.href : String(filename);
      if (!value.startsWith("file:///") && !path.isAbsolute(value)) {
        throw Object.assign(
          new TypeError("The argument 'filename' must be a file URL object, file URL string, or absolute path string"),
          { code: "ERR_INVALID_ARG_VALUE" }
        );
      }
      const directory = path.dirname(value.startsWith("file:///") ? decodeURIComponent(new URL(value).pathname) : value);
      const created = (specifier) => {
        const request = String(specifier);
        return request.startsWith(".")
          ? globalThis.require(path.resolve(directory, request))
          : globalThis.require(request);
      };
      return created;
    };
    if (!moduleApi.findPackageJSON) {
      moduleApi.findPackageJSON = (specifier, base = process.cwd()) => {
        if (typeof specifier !== "string") {
          const error = new TypeError("The \"specifier\" argument must be of type string");
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        const path = globalThis.require("path");
        const fs = globalThis.require("fs");
        let candidate = path.resolve(base, specifier);
        if (fs.existsSync(candidate) && !fs.statSync(candidate).isDirectory()) candidate = path.dirname(candidate);
        while (true) {
          const result = path.join(candidate, "package.json");
          if (fs.existsSync(result)) return result;
          const parent = path.dirname(candidate);
          if (parent === candidate) return undefined;
          candidate = parent;
        }
      };
    }
    for (const name of "findSourceMap syncBuiltinESMExports register runMain setSourceMapsSupport flushCompileCache getCompileCacheDir".split(
      " "
    )) {
      moduleApi[name] ||= () => undefined;
    }
    moduleApi.registerHooks ||= () => ({});
    moduleApi.getSourceMapsSupport ||= () => ({});
    moduleApi.stripTypeScriptTypes ||= (source) => String(source);
    moduleApi.enableCompileCache ||= () => ({});
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
"#);
