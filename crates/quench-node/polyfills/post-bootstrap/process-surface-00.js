{
  if (globalThis.process) {
    globalThis.process[Symbol.toStringTag] ||= "process";
    globalThis.gc ||= () => undefined;
    globalThis.process.emitWarning = (message, options = {}) => {
      const warning =
        message instanceof Error ? message : new Error(String(message));
      const settings =
        typeof options === "string" ? { name: options } : options || {};
      warning.name = String(settings.name || "Warning");
      if (settings.code !== undefined) warning.code = settings.code;
      globalThis.process.emit("warning", warning);
      return undefined;
    };
    const activeTimers = new Map();
    const originalSetTimeout = globalThis.setTimeout;
    const originalClearTimeout = globalThis.clearTimeout;
    const originalSetInterval = globalThis.setInterval;
    const originalClearInterval = globalThis.clearInterval;
    const originalSetImmediate = globalThis.setImmediate;
    const originalClearImmediate = globalThis.clearImmediate;
    if (typeof originalSetTimeout === "function") {
      globalThis.setTimeout = (callback, delay, ...args) => {
        if (typeof callback !== "function") {
          throw new TypeError(
            'The "callback" argument must be of type function'
          );
        }
        let timer;
        const wrappedCallback = (...callbackArgs) => {
          try {
            return callback(...callbackArgs);
          } finally {
            activeTimers.delete(timer);
          }
        };
        timer = originalSetTimeout(wrappedCallback, delay, ...args);
        activeTimers.set(timer, "Timeout");
        return timer;
      };
      globalThis.clearTimeout = (timer) => {
        activeTimers.delete(timer);
        return originalClearTimeout(timer);
      };
    }
    if (typeof originalSetInterval === "function") {
      globalThis.setInterval = (callback, delay, ...args) => {
        if (typeof callback !== "function") {
          throw new TypeError(
            'The "callback" argument must be of type function'
          );
        }
        const timer = originalSetInterval(callback, delay, ...args);
        activeTimers.set(timer, "Timeout");
        return timer;
      };
      globalThis.clearInterval = (timer) => {
        activeTimers.delete(timer);
        return originalClearInterval(timer);
      };
    }
    if (typeof originalSetImmediate === "function") {
      globalThis.setImmediate = (callback, ...args) => {
        if (typeof callback !== "function") {
          throw new TypeError(
            'The "callback" argument must be of type function'
          );
        }
        let timer;
        const wrappedCallback = (...callbackArgs) => {
          activeTimers.delete(timer);
          return callback(...callbackArgs);
        };
        timer = originalSetImmediate(wrappedCallback, ...args);
        activeTimers.set(timer, "Immediate");
        return timer;
      };
      globalThis.clearImmediate = (timer) => {
        activeTimers.delete(timer);
        return originalClearImmediate(timer);
      };
    }
    globalThis.process.getActiveResourcesInfo = () => [
      ...activeTimers.values()
    ];
    globalThis.process.availableMemory = () => Number.MAX_SAFE_INTEGER;
    globalThis.process.setSourceMapsEnabled = () => undefined;
    globalThis.process.sourceMapsEnabled = false;
    globalThis.process.debugPort = 9229;
    globalThis.process.release = {
      name: "node",
      sourceUrl: "",
      headersUrl: ""
    };
    const allowedFlags = new Set([
      "--perf_basic_prof",
      "--perf-basic-prof",
      "--perf_basic-prof",
      "-r",
      "--stack-trace-limit",
      "--inspect-brk"
    ]);
    const allowedFlagsHas = allowedFlags.has.bind(allowedFlags);
    allowedFlags.has = (flag) => {
      if (flag === "perf-basic-prof" || flag === "perf_basic-prof") return true;
      if (flag === "perf_basic_prof" || flag === "r") return true;
      if (flag === "inspect-brk" || flag === "--inspect_brk") return true;
      return (
        allowedFlagsHas(flag) ||
        (typeof flag === "string" && flag.startsWith("--stack-trace-limit="))
      );
    };
    const protectedSets = (globalThis.__quenchProtectedSets ||= new WeakSet());
    protectedSets.add(allowedFlags);
    if (!globalThis.__quenchProtectedSetMethods) {
      const originalAdd = Set.prototype.add;
      const originalDelete = Set.prototype.delete;
      const originalClear = Set.prototype.clear;
      Set.prototype.add = function (value) {
        return protectedSets.has(this) ? this : originalAdd.call(this, value);
      };
      Set.prototype.delete = function (value) {
        return protectedSets.has(this)
          ? false
          : originalDelete.call(this, value);
      };
      Set.prototype.clear = function () {
        if (!protectedSets.has(this)) originalClear.call(this);
      };
      globalThis.__quenchProtectedSetMethods = true;
    }
    globalThis.process.allowedNodeEnvironmentFlags =
      Object.freeze(allowedFlags);
    globalThis.process.execArgv = [];
    globalThis.process.argv0 = "node";
    globalThis.process.features ||= {};
    globalThis.process.features.inspector ??= false;
    globalThis.process.noDeprecation ??= false;
    globalThis.process.traceDeprecation ??= false;
    globalThis.process.throwDeprecation ??= false;
    globalThis.process.version ||= "v22.0.0";
    globalThis.process.versions ||= {};
    globalThis.process.versions.node ??= "22.0.0";
    globalThis.process.versions.v8 ??= "12.4.254.21-node.20";
    globalThis.process.versions.uv ??= "1.48.0";
    globalThis.process.versions.openssl ??= "3.0.13";
    globalThis.process.versions.zlib ??= "1.3.0";
    globalThis.process.versions.modules ??= "127";
    globalThis.process.versions.napi ??= "9";
    globalThis.process.versions.acorn ??= "8.11.3";
    globalThis.process.versions.ada ??= "2.7.8";
    globalThis.process.versions.tz ??= "2024a";
    globalThis.process.versions.brotli ??= "1.1.0";
    globalThis.process.versions.nbytes ??= "1.0.0";
    globalThis.process.versions.cldr ??= "45.0";
    globalThis.process.versions.icu ??= "75.1";
    globalThis.process.versions.nghttp2 ??= "1.61.0";
    globalThis.process.versions.llhttp ??= "9.2.1";
    globalThis.process.versions.nghttp3 ??= "1.3.0";
    globalThis.process.versions.ngtcp2 ??= "1.4.0";
    globalThis.process.versions.simdutf ??= "5.2.4";
    globalThis.process.versions.unicode ??= "15.1";
    globalThis.process.versions.undici ??= "6.19.8";
    globalThis.process.versions.cjs_module_lexer ??= "1.2.2";
    globalThis.process.title =
      globalThis.__quench_cli_title || globalThis.process.title || "node";
    globalThis.process.getBuiltinModule ||= (name) =>
      globalThis.require(String(name).replace(/^node:/, ""));
    globalThis.process.loadEnvFile ||= () => undefined;
    globalThis.process.finalization ||= {
      register: () => undefined,
      unregister: () => undefined,
      registerBeforeExit: () => undefined
    };
    globalThis.process.permission ||= { has: () => false };
    globalThis.process.resourceUsage ||= () => ({
      userCPUTime: 0,
      systemCPUTime: 0,
      maxRSS: 0,
      minorPageFault: 0,
      majorPageFault: 0,
      fsRead: 0,
      fsWrite: 0,
      involuntaryContextSwitches: 0,
      voluntaryContextSwitches: 0
    });
    globalThis.process.cpuUsage = (previous) => {
      if (previous === undefined) return { user: 0, system: 0 };
      if (!previous || typeof previous !== "object") {
        const error = new TypeError(
          `The "prevValue" argument must be of type object. Received type ${typeof previous} (${String(
            previous
          )})`
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      for (const field of ["user", "system"]) {
        const value = previous[field];
        if (typeof value !== "number") {
          const received =
            value == null
              ? ` Received ${value}`
              : ` Received type ${typeof value} (${String(value)})`;
          const error = new TypeError(
            `The "prevValue.${field}" property must be of type number.` +
              received
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (!Number.isFinite(value) || value < 0) {
          const error = new RangeError(
            `The property 'prevValue.${field}' is invalid. Received ${value}`
          );
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
      }
      return { user: 0, system: 0 };
    };
    globalThis.process.memoryUsage.rss ||= () => 0;
  }
}
