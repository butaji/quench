//! Polyfill: `process-surface-00`

pub const JS: &str = quench_js_check::checked_js!(r#"{
  if (globalThis.process) {
    globalThis.process[Symbol.toStringTag] ||= "process";
    globalThis.gc ||= () => undefined;
    const activeTimers = new Map();
    const originalSetTimeout = globalThis.setTimeout;
    const originalClearTimeout = globalThis.clearTimeout;
    const originalSetInterval = globalThis.setInterval;
    const originalClearInterval = globalThis.clearInterval;
    const originalSetImmediate = globalThis.setImmediate;
    const originalClearImmediate = globalThis.clearImmediate;
    const validateTimerCallback = (callback) => {
      if (typeof callback !== "function") {
        throw new TypeError('The "callback" argument must be of type function');
      }
    };
    if (typeof originalSetTimeout === "function") {
      globalThis.setTimeout = (callback, delay, ...args) => {
        validateTimerCallback(callback);
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
        validateTimerCallback(callback);
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
        validateTimerCallback(callback);
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
    globalThis.process.constrainedMemory ||= () => Number.MAX_SAFE_INTEGER;
    globalThis.process.setSourceMapsEnabled = () => undefined;
    globalThis.process.sourceMapsEnabled = false;
    globalThis.process.debugPort = 9229;
    globalThis.process.release = {
      name: "node",
      sourceUrl: "",
      headersUrl: ""
    };
    if (!(globalThis.process.allowedNodeEnvironmentFlags instanceof Set)) {
    const allowedFlags = new Set(
      "--perf_basic_prof --perf-basic-prof --perf_basic-prof -r --stack-trace-limit --inspect-brk".split(
        " "
      )
    );
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
    const protectedSets = new WeakSet();
    protectedSets.add(allowedFlags);
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
    globalThis.process.allowedNodeEnvironmentFlags =
      Object.freeze(allowedFlags);
    }
    if (globalThis.__quench_allowed_node_environment_flags instanceof Set) {
      globalThis.process.allowedNodeEnvironmentFlags =
        globalThis.__quench_allowed_node_environment_flags;
    }
    // The Rust host installs invocation flags before bootstrap. Preserve that
    // fact; only supply the empty default for embedders that omit it.
    globalThis.process.execArgv ??= [];
    globalThis.process.argv0 ||= "node";
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
    globalThis.process.getBuiltinModule ||= (name) => {
      if (typeof name !== "string") {
        const received = name === null ? "Received null" : name === undefined
          ? "Received undefined"
          : typeof name === "object"
            ? `Received an instance of ${Array.isArray(name) ? "Array" : "Object"}`
            : `Received type ${typeof name} (${String(name)})`;
        throw Object.assign(
          new TypeError(`The "id" argument must be of type string. ${received}`),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
      const builtin = name.replace(/^node:/, "");
      const builtinNames = globalThis["\0quench:require"]("module").builtinModules;
      if (
        !builtinNames.includes(name) &&
        !(name.startsWith("node:") && builtinNames.includes(builtin))
      ) return undefined;
      try {
        return globalThis["\0quench:require"](name);
      } catch (error) {
        if (error?.code === "MODULE_NOT_FOUND" || error?.code === "ERR_UNKNOWN_BUILTIN_MODULE") {
          return undefined;
        }
        throw error;
      }
    };
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
    globalThis.process.memoryUsage ||= () => ({
      rss: 0,
      heapTotal: 0,
      heapUsed: 0,
      external: 0,
      arrayBuffers: 0
    });
    globalThis.process.memoryUsage.rss ||= () =>
      globalThis.process.memoryUsage().rss;
    const cpuUsage = (previous) => {
      const usage = { user: 0, system: 0 };
      if (previous === undefined) return usage;
      if (previous === null || typeof previous !== "object" || Array.isArray(previous)) {
        const received = previous === null ? "Received null" :
          typeof previous === "string" ? `Received type string (${previous})` :
          typeof previous === "number" ? `Received type number (${previous})` :
          typeof previous === "boolean" ? `Received type boolean (${previous})` :
          Array.isArray(previous) ? "Received an instance of Array" :
          `Received type ${typeof previous}`;
        throw Object.assign(
          new TypeError(`The "prevValue" argument must be of type object. ${received}`),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
      for (const field of ["user", "system"]) {
        if (typeof previous[field] !== "number") {
          const value = previous[field];
          const received = value === undefined ? "Received undefined" :
            value === null ? "Received null" :
            typeof value === "object" ? `Received an instance of ${Array.isArray(value) ? "Array" : "Object"}` :
            `Received type ${typeof value} (${typeof value === "string" ? `'${value}'` : String(value)})`;
          throw Object.assign(
            new TypeError(`The "prevValue.${field}" property must be of type number. ${received}`),
            { code: "ERR_INVALID_ARG_TYPE" }
          );
        }
        if (!Number.isFinite(previous[field]) || previous[field] < 0) {
          throw Object.assign(
            new RangeError(`The property 'prevValue.${field}' is invalid. Received ${previous[field]}`),
            { code: "ERR_INVALID_ARG_VALUE" }
          );
        }
        usage[field] = Math.max(0, usage[field] - previous[field]);
      }
      return usage;
    };
    globalThis.process.cpuUsage ||= cpuUsage;
    globalThis.process.threadCpuUsage ||= cpuUsage;
    globalThis.process.ref ||= (target) => {
      const ref = target?.[Symbol.for("nodejs.ref")] || target?.ref;
      if (typeof ref === "function") ref.call(target);
      return target;
    };
    globalThis.process.unref ||= (target) => {
      const unref = target?.[Symbol.for("nodejs.unref")] || target?.unref;
      if (typeof unref === "function") unref.call(target);
      return target;
    };
  }
}
"#);
