//! Polyfill: `module-surface-07`

pub const JS: &str = r#"{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const normalized = String(name).replace(/^node:/, "");
      if (normalized === "wasi") {
        return {
          WASI: function WASI() {},
          getImportObject: () => ({}),
          WASI_VERSION: " wasi_snapshot_preview1",
          WASI_PREVIEW1: " wasi_snapshot_preview1"
        };
      }
      if (normalized === "trace_events") {
        return {
          createTracing: (options) => {
            let enabled = Boolean(options?.enabled);
            return {
              get enabled() {
                return enabled;
              },
              enable: () => {
                enabled = true;
              },
              disable: () => {
                enabled = false;
              },
              categories: (options?.categories || []).join(",")
            };
          },
          getEnabledCategories: () => ""
        };
      }
      let result = originalRequire(name);
      if (normalized === "timers/promises") {
        result.scheduler ||= {
          wait: async () => undefined,
          yield: async () => undefined
        };
      }
      if (normalized === "perf_hooks") {
        for (const name of "PerformanceEntry PerformanceMark PerformanceMeasure".split(
          " "
        )) {
          result[name] ||= function Constructor() {};
        }
        result.monitorEventLoopDelay ||= () => ({});
        result.createHistogram ||= () => ({});
        result.constants ||= {};
      }
      if (normalized === "https") {
        result = Object.assign({}, result);
        for (const name of "request get createServer".split(" ")) {
          result[name] ||= () => undefined;
        }
        result.Agent ||= function Agent() {};
        result.Server ||= function Server() {};
        result.globalAgent ||= {};
      }
      if (normalized === "repl") {
        result = Object.assign({}, result);
        result.start ||= () => ({});
        result.recoverable ||= () => false;
        result.REPLServer ||= function REPLServer() {};
      }
      if (normalized === "v8") {
        result = Object.assign({}, result);
        result.serialize ||= (value) => value;
        result.deserialize ||= (value) => value;
        result.getHeapStatistics ||= () => ({});
        result.getHeapSpaceStatistics ||= () => [];
        result.getHeapCodeStatistics ||= () => ({});
        result.setFlagsFromString ||= () => undefined;
        result.cachedDataVersionTag ||= () => 0;
      }
      if (normalized === "vm") {
        result = Object.assign({}, result);
        result.runInContext ||= () => undefined;
        result.runInNewContext ||= () => undefined;
        result.runInThisContext ||= () => undefined;
        result.createContext ||= () => ({});
        result.isContext ||= () => false;
        result.compileFunction ||= () => () => undefined;
        for (const constructor of "Script Context Module SourceTextModule SyntheticModule".split(
          " "
        )) {
          result[constructor] ||= function Constructor() {};
        }
      }
      if (normalized === "async_hooks") {
        result.createHook ||= () => ({
          enable: () => undefined,
          disable: () => undefined
        });
        result.executionAsyncId ||= () => 0;
        result.triggerAsyncId ||= () => 0;
        result.executionAsyncResource ||= () => ({});
        result.AsyncResource ||= function AsyncResource() {};
        result.AsyncLocalStorage ||= globalThis.__nodeAsyncLocalStorage;
      }
      if (normalized === "constants") {
        result = Object.assign({}, result);
        result.errno ||= {};
        result.signals ||= {};
        result.os ||= {};
        result.fs ||= {};
        result.crypto ||= {};
        result.zlib ||= {};
        result.O_RDONLY ??= 0;
        result.SIGTERM ??= 15;
        result = Object.freeze(result);
      }
      return result;
    };
  }
}
"#;
