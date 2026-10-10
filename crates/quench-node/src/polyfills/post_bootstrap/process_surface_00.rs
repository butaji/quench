//! Polyfill: `process-surface-00`

pub const JS: &str = quench_js_check::checked_js!(r#"{
  if (globalThis.process) {
    globalThis.process[Symbol.toStringTag] ||= "process";
    if (globalThis.__nodeEventEmitter?.prototype) {
      const Process = function process() {};
      Object.setPrototypeOf(Process.prototype, globalThis.__nodeEventEmitter.prototype);
      Object.setPrototypeOf(globalThis.process, Process.prototype);
    }
    const processEnv = globalThis.process.env;
    if (processEnv) {
      globalThis.process.env = new Proxy(processEnv, {
        set(target, key, value) {
          if (typeof key === "symbol" || typeof value === "symbol") {
            throw new TypeError("Cannot convert a Symbol value to a string");
          }
          if (key === "") return true;
          if (typeof value !== "string" && typeof value !== "number" &&
              typeof value !== "boolean" &&
              globalThis.process.execArgv.includes("--pending-deprecation")) {
            globalThis.process.emitWarning(
              "Assigning any value other than a string, number, or boolean to a process.env property is deprecated. Please make sure to convert the value to a string before setting process.env with it.",
              { type: "DeprecationWarning", code: "DEP0104" }
            );
          }
          return Reflect.set(target, key, String(value), target);
        },
        defineProperty(target, key, descriptor) {
          const invalid = (message) => {
            const error = new TypeError(message);
            error.code = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";
            return error;
          };
          if (typeof key === "symbol") {
            throw invalid("'process.env' does not accept symbol properties");
          }
          if ("get" in descriptor || "set" in descriptor) {
            throw invalid("'process.env' does not accept an accessor(getter/setter) descriptor");
          }
          if (descriptor.configurable !== true || descriptor.writable !== true || descriptor.enumerable !== true) {
            throw invalid("'process.env' only accepts a configurable, writable, and enumerable data descriptor");
          }
          if (typeof descriptor.value === "symbol") {
            throw new TypeError("Cannot convert a Symbol value to a string");
          }
          return Reflect.defineProperty(target, key, {
            value: String(descriptor.value),
            configurable: true,
            writable: true,
            enumerable: true
          });
        }
      });
    }
    if (typeof globalThis.process.emitWarning === "function") {
      const emitWarning = globalThis.process.emitWarning.bind(globalThis.process);
      globalThis.process.emitWarning = (warning, type, code) => {
        let message;
        let options = {};
        if (typeof warning === "string") {
          message = warning;
        } else if (warning instanceof Error) {
          message = warning.message;
          options.type = warning.name;
          if (typeof warning.code === "string") options.code = warning.code;
          if (typeof warning.detail === "string") options.detail = warning.detail;
        } else {
          const received = warning === undefined ? "undefined" :
            warning === null ? "null" : `an instance of ${warning?.constructor?.name || typeof warning}`;
          const error = new TypeError(`The "warning" argument must be of type string or an instance of Error. Received ${received}`);
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (typeof type === "string") {
          options.type = type;
          if (typeof code === "string") options.code = code;
          else if (code !== undefined && typeof code !== "function") {
            const error = new TypeError('The "code" argument must be of type string.');
            error.code = "ERR_INVALID_ARG_TYPE";
            throw error;
          }
        } else if (typeof type === "function") {
          options.type = type.name;
        } else if (type && typeof type === "object" && !Array.isArray(type)) {
          options = type;
        } else if (type !== undefined) {
          const error = new TypeError('The "type" argument must be of type string or an object.');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (options.type === "DeprecationWarning" && globalThis.process.noDeprecation) {
          return undefined;
        }
        if (options.type === "DeprecationWarning" && globalThis.process.throwDeprecation) {
          const error = new Error(message);
          error.name = options.type;
          if (typeof options.code === "string") error.code = options.code;
          if (typeof options.detail === "string") error.detail = options.detail;
          return globalThis.process.nextTick(() => { throw error; });
        }
        return emitWarning(message, options);
      };
    }
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
    globalThis.process.setSourceMapsEnabled = (enabled) => {
      if (typeof enabled !== "boolean") {
        const received = enabled === null ? "Received null" :
          enabled === undefined ? "Received undefined" :
          typeof enabled === "object" ? `Received an instance of ${Array.isArray(enabled) ? "Array" : "Object"}` :
          `Received type ${typeof enabled} (${String(enabled)})`;
        throw Object.assign(
          new TypeError(`[ERR_INVALID_ARG_TYPE] The "enabled" argument must be of type boolean. ${received}`),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
      globalThis.process.sourceMapsEnabled = enabled;
    };
    globalThis.process.abort ||= () => globalThis.process.exit(134);
    globalThis.process.sourceMapsEnabled = false;
    globalThis.process.debugPort = 9229;
    globalThis.process.release = {
      name: "node",
      sourceUrl: "",
      headersUrl: ""
    };
    if (!(globalThis.process.allowedNodeEnvironmentFlags instanceof Set)) {
    const nodeAllowedFlags = "--abort-on-uncaught-exception --addons --allow-addons --allow-child-process --allow-fs-read --allow-fs-write --allow-inspector --allow-net --allow-openssl-store --allow-wasi --allow-worker --async-context-frame --conditions --debug-arraybuffer-allocations --deprecation --diagnostic-dir --disable-proto --disable-sigusr1 --disable-warning --disable-wasm-trap-handler --disallow-code-generation-from-strings --dns-result-order --enable-etw-stack-walking --enable-fips --enable-network-family-autoselection --enable-source-maps --entry-url --es-module-specifier-resolution --experimental-abortcontroller --experimental-addon-modules --experimental-detect-module --experimental-dtls --experimental-eventsource --experimental-fetch --experimental-global-customevent --experimental-global-navigator --experimental-global-webcrypto --experimental-import-meta-resolve --experimental-import-text --experimental-json-modules --experimental-loader --experimental-modules --experimental-package-map --experimental-print-required-tla --experimental-quic --experimental-repl-await --experimental-report --experimental-require-module --experimental-shadow-realm --experimental-specifier-resolution --experimental-sqlite --experimental-stream-iter --experimental-strip-types --experimental-test-isolation --experimental-top-level-await --experimental-vfs --experimental-vm-modules --experimental-wasi-unstable-preview1 --experimental-wasm-modules --experimental-web-worker --experimental-websocket --experimental-webstorage --experimental-worker --expose-gc --extra-info-on-fatal-exception --force-async-hooks-checks --force-context-aware --force-fips --force-node-api-uncaught-exceptions-policy --frozen-intrinsics --global-search-paths --heapsnapshot-near-heap-limit --heapsnapshot-signal --http-parser --icu-data-dir --import --input-type --insecure-http-parser --interpreted-frames-native-stack --jitless --loader --localstorage-file --max-heap-size --max-http-header-size --max-old-space-size --max-old-space-size-percentage --max-semi-space-size --napi-modules --network-family-autoselection --network-family-autoselection-attempt-timeout --no-addons --no-allow-addons --no-allow-child-process --no-allow-inspector --no-allow-wasi --no-allow-worker --no-async-context-frame --no-debug-arraybuffer-allocations --no-deprecation --no-disable-sigusr1 --no-disable-wasm-trap-handler --no-enable-fips --no-enable-source-maps --no-entry-url --no-experimental-addon-modules --no-experimental-detect-module --no-experimental-eventsource --no-experimental-global-navigator --no-experimental-import-meta-resolve --no-experimental-import-text --no-experimental-print-required-tla --no-experimental-repl-await --no-experimental-require-module --no-experimental-shadow-realm --no-experimental-sqlite --no-experimental-vm-modules --no-experimental-websocket --no-experimental-webstorage --no-extra-info-on-fatal-exception --no-force-async-hooks-checks --no-force-context-aware --no-force-fips --no-force-node-api-uncaught-exceptions-policy --no-frozen-intrinsics --no-global-search-paths --no-insecure-http-parser --no-network-family-autoselection --no-node-snapshot --no-openssl-legacy-provider --no-openssl-shared-config --no-pending-deprecation --no-permission --no-preserve-symlinks --no-preserve-symlinks-main --no-report-compact --no-report-exclude-env --no-report-exclude-network --no-report-on-fatalerror --no-report-on-signal --no-report-uncaught-exception --no-require-module --no-strip-types --no-test-only --no-test-randomize --no-throw-deprecation --no-tls-max-v1.2 --no-tls-max-v1.3 --no-tls-min-v1.0 --no-tls-min-v1.1 --no-tls-min-v1.2 --no-tls-min-v1.3 --no-trace-deprecation --no-trace-env --no-trace-env-js-stack --no-trace-env-native-stack --no-trace-exit --no-trace-promises --no-trace-sigint --no-trace-sync-io --no-trace-tls --no-trace-uncaught --no-trace-warnings --no-track-heap-objects --no-use-bundled-ca --no-use-env-proxy --no-use-openssl-ca --no-use-system-ca --no-verify-base-objects --no-warnings --no-watch --no-watch-preserve-output --no-zero-fill-buffers --node-memory-debug --node-snapshot --openssl-config --openssl-legacy-provider --openssl-shared-config --pending-deprecation --perf-basic-prof --perf-basic-prof-only-functions --perf-prof --perf-prof-unwinding-info --permission --permission-audit --preserve-symlinks --preserve-symlinks-main --prof-process --redirect-warnings --report-compact --report-dir --report-directory --report-exclude-env --report-exclude-network --report-filename --report-on-fatalerror --report-on-signal --report-signal --report-uncaught-exception --require --require-module --secure-heap --secure-heap-min --snapshot-blob --stack-trace-limit --strip-types --test-coverage-branches --test-coverage-exclude --test-coverage-functions --test-coverage-include --test-coverage-include-all --test-coverage-lines --test-global-setup --test-isolation --test-name-pattern --test-only --test-random-seed --test-randomize --test-reporter --test-reporter-destination --test-rerun-failures --test-shard --test-skip-pattern --throw-deprecation --title --tls-cipher-list --tls-keylog --tls-max-v1.2 --tls-max-v1.3 --tls-min-v1.0 --tls-min-v1.1 --tls-min-v1.2 --tls-min-v1.3 --trace-deprecation --trace-env --trace-env-js-stack --trace-env-native-stack --trace-event-categories --trace-event-file-pattern --trace-events-enabled --trace-exit --trace-promises --trace-require-module --trace-sigint --trace-sync-io --trace-tls --trace-uncaught --trace-warnings --track-heap-objects --unhandled-rejections --use-bundled-ca --use-env-proxy --use-largepages --use-openssl-ca --use-system-ca --v8-pool-size --verify-base-objects --warnings --watch --watch-kill-signal --watch-path --watch-preserve-output --webstorage --zero-fill-buffers -C -r".split(" ");
    const availableFlags = nodeAllowedFlags;
    const allowedFlags = new Set(availableFlags);
    const allowedFlagsHas = allowedFlags.has.bind(allowedFlags);
    const flagsWithoutLeadingDashes = new Set(
      availableFlags.map((flag) => flag.replace(/^--?/, ""))
    );
    allowedFlags.has = (flag) => {
      if (typeof flag !== "string") return false;
      const normalized = flag.replaceAll("_", "-");
      if (normalized.startsWith("-")) {
        return allowedFlagsHas(normalized.replace(/=.*$/, ""));
      }
      return flagsWithoutLeadingDashes.has(normalized);
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
    delete globalThis.__quench_cli_title;
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
