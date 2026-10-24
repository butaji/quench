//! Polyfill: `process-surface-00`

pub const JS: &str = quench_js_check::checked_js!(r#"{
  if (globalThis.process) {
    const disableProto = globalThis.process.execArgv?.find((argument) =>
      argument === "--disable-proto=delete" || argument === "--disable-proto=throw" ||
      argument === "--disable-proto" && globalThis.process.execArgv[globalThis.process.execArgv.indexOf(argument) + 1] !== undefined
    );
    if (disableProto) {
      const mode = disableProto.includes("=")
        ? disableProto.slice(disableProto.indexOf("=") + 1)
        : globalThis.process.execArgv[globalThis.process.execArgv.indexOf(disableProto) + 1];
      if (mode === "delete") {
        Reflect.deleteProperty(Object.prototype, "__proto__");
      } else if (mode === "throw") {
        const throwProtoAccess = () => {
          const error = new Error("Accessing Object.prototype.__proto__ is disabled");
          error.code = "ERR_PROTO_ACCESS";
          throw error;
        };
        Reflect.defineProperty(Object.prototype, "__proto__", {
          get: throwProtoAccess,
          set: throwProtoAccess,
          configurable: true,
          enumerable: false,
        });
      }
    }
    globalThis.process[Symbol.toStringTag] ||= "process";
    if (globalThis.__nodeEventEmitter?.prototype) {
      const Process = function process() {};
      Object.setPrototypeOf(Process.prototype, globalThis.__nodeEventEmitter.prototype);
      Object.setPrototypeOf(globalThis.process, Process.prototype);
    }
    globalThis.process.kill ||= (pid, signal) => {
      const numericPid = typeof pid === "number" ? pid :
        typeof pid === "string" && pid !== "" ? Number(pid) : NaN;
      if (!Number.isInteger(numericPid) || numericPid < -2147483648 ||
          numericPid > 2147483647) {
        const received = pid === null ? "null" : pid === undefined ? "undefined" :
          typeof pid === "string" ? `type string ('${pid}')` :
          typeof pid === "number" ? `type number (${String(pid)})` :
          `an instance of ${pid.constructor?.name || typeof pid}`;
        const error = new TypeError(`The "pid" argument must be of type number. Received ${received}`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const signalNumbers = { SIGHUP: 1, SIGINT: 2, SIGQUIT: 3, SIGKILL: 9, SIGTERM: 15 };
      let number;
      if (Number.isInteger(signal)) {
        number = signal;
      } else {
        const name = signal || "SIGTERM";
        number = signalNumbers[name];
        if (number === undefined) {
          const error = new TypeError(`Unknown signal: ${name}`);
          error.code = "ERR_UNKNOWN_SIGNAL";
          throw error;
        }
      }
      const errorCode = globalThis.process._kill(pid, number);
      if (errorCode) {
        const errorNames = { 1: "EPERM", 2: "ENOENT", 3: "ESRCH", 13: "EACCES", 22: "EINVAL" };
        const code = errorNames[errorCode] || "UNKNOWN";
        const error = new Error(`kill ${code}`);
        error.code = code;
        throw error;
      }
      return true;
    };
    const processEnv = globalThis.process.env;
    if (processEnv) {
      const setEnv = globalThis.process.__quenchSetEnv;
      const deleteEnv = globalThis.process.__quenchDeleteEnv;
      const getSharedEnv = globalThis.process.__quenchGetSharedEnv;
      Object.defineProperty(globalThis.process, "__quenchSetEnv", { enumerable: false });
      Object.defineProperty(globalThis.process, "__quenchDeleteEnv", { enumerable: false });
      Object.defineProperty(globalThis.process, "__quenchGetSharedEnv", { enumerable: false });
      Object.defineProperty(globalThis.process, "__quenchEnvShared", { enumerable: false });
      globalThis.process.env = new Proxy(processEnv, {
        get(target, key, receiver) {
          if (globalThis.process.__quenchEnvShared && typeof key === "string") {
            return getSharedEnv(key);
          }
          return Reflect.get(target, key, receiver);
        },
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
          const text = String(value);
          const result = Reflect.defineProperty(target, key, {
            value: text,
            configurable: true,
            writable: true,
            enumerable: true
          });
          if (result && typeof key !== "symbol") setEnv(String(key), text);
          return result;
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
          if (key === "") return true;
          if ("get" in descriptor || "set" in descriptor) {
            throw invalid("'process.env' does not accept an accessor(getter/setter) descriptor");
          }
          if (descriptor.configurable !== true || descriptor.writable !== true || descriptor.enumerable !== true) {
            throw invalid("'process.env' only accepts a configurable, writable, and enumerable data descriptor");
          }
          if (typeof descriptor.value === "symbol") {
            throw new TypeError("Cannot convert a Symbol value to a string");
          }
          const value = String(descriptor.value);
          const result = Reflect.defineProperty(target, key, {
            value,
            configurable: true,
            writable: true,
            enumerable: true
          });
          if (result) setEnv(String(key), value);
          return result;
        },
        deleteProperty(target, key) {
          const result = Reflect.deleteProperty(target, key);
          if (result && key !== "" && typeof key !== "symbol") deleteEnv(String(key));
          return result;
        }
      });
    }
    // The Rust host installs invocation flags before bootstrap. Preserve that
    // fact; only supply the empty default for embedders that omit it.
    globalThis.process.execArgv ??= [];
    globalThis.process.argv0 ||= "node";
    globalThis.process.features ||= {};
    globalThis.process.features.inspector ??= false;
    // Node's startup coverage hook warns when NODE_V8_COVERAGE is requested
    // without the inspector profiler, then clears the variable so it is not
    // inherited by further children as a second failed request.
    if (globalThis.process.env?.NODE_V8_COVERAGE &&
        !globalThis.process.features.inspector) {
      globalThis.process.emitWarning(
        'The inspector is disabled, coverage could not be collected',
        'Warning',
      );
      globalThis.process.env.NODE_V8_COVERAGE = '';
    }
    globalThis.process.title =
      globalThis.__quench_cli_title || globalThis.process.title || "node";
    delete globalThis.__quench_cli_title;
    globalThis.process.loadEnvFile ||= () => undefined;
  }
}
"#);

/// The `process` members `LAZY_JS` defines, which are absent until it runs: a lazy group
/// (`define_lazy_data_properties_rooted`), so the script is compiled on first use.
pub const LAZY_MEMBERS: &[&str] = &[
    "getActiveResourcesInfo",
    "_getActiveRequests",
    "availableMemory",
    "constrainedMemory",
    "setSourceMapsEnabled",
    "abort",
    "sourceMapsEnabled",
    "debugPort",
    "release",
    "allowedNodeEnvironmentFlags",
    "getBuiltinModule",
    "finalization",
    "permission",
    "resourceUsage",
    "memoryUsage",
    "cpuUsage",
    "threadCpuUsage",
    "ref",
    "unref",
];

pub const LAZY_JS: &str = quench_js_check::checked_js!(r#"{
  if (globalThis.process) {
    globalThis.process.getActiveResourcesInfo = () => [
      ...globalThis.process.__quenchGetActiveTimerResources(),
      ...globalThis.process.__quenchGetActiveNetworkResources(),
      ...Array.from(
        globalThis.process.__quenchActiveRequests || [],
        () => "FSReqCallback"
      )
    ];
    globalThis.process._getActiveRequests = () =>
      globalThis.process.__quenchActiveRequests || [];
    globalThis.process.availableMemory ||= () => Number.MAX_SAFE_INTEGER;
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
    // Like Node, the set is built on first read: a frozen Set subclass whose mutators do
    // nothing, so Set.prototype itself stays untouched.
    const hostAllowedFlags = globalThis.__quench_allowed_node_environment_flags;
    const buildAllowedFlags = () => {
      if (hostAllowedFlags instanceof Set) return hostAllowedFlags;
      const availableFlags = "--abort-on-uncaught-exception --addons --allow-addons --allow-child-process --allow-fs-read --allow-fs-vfs --allow-fs-write --allow-inspector --allow-net --allow-openssl-store --allow-wasi --allow-worker --async-context-frame --bench-isolation --bench-name-pattern --bench-reporter --bench-reporter-destination --bench-samples --bench-warmup --conditions --debug-arraybuffer-allocations --deprecation --diagnostic-dir --disable-proto --disable-sigusr1 --disable-warning --disable-wasm-trap-handler --disallow-code-generation-from-strings --dns-result-order --enable-etw-stack-walking --enable-fips --enable-fips-indicator-events --enable-network-family-autoselection --enable-source-maps --entry-url --es-module-specifier-resolution --experimental-abortcontroller --experimental-addon-modules --experimental-bench --experimental-detect-module --experimental-dtls --experimental-eventsource --experimental-fetch --experimental-global-customevent --experimental-global-navigator --experimental-global-webcrypto --experimental-import-meta-resolve --experimental-import-text --experimental-json-modules --experimental-loader --experimental-modules --experimental-package-map --experimental-print-required-tla --experimental-quic --experimental-repl-await --experimental-report --experimental-require-module --experimental-shadow-realm --experimental-specifier-resolution --experimental-sqlite --experimental-stream-iter --experimental-strip-types --experimental-test-isolation --experimental-top-level-await --experimental-vfs --experimental-vm-modules --experimental-wasi-unstable-preview1 --experimental-wasm-modules --experimental-web-worker --experimental-websocket --experimental-webstorage --experimental-worker --expose-gc --extra-info-on-fatal-exception --force-async-hooks-checks --force-context-aware --force-fips --force-node-api-uncaught-exceptions-policy --frozen-intrinsics --global-search-paths --heapsnapshot-near-heap-limit --heapsnapshot-signal --http-parser --icu-data-dir --import --input-type --insecure-http-parser --interpreted-frames-native-stack --jitless --loader --localstorage-file --max-heap-size --max-http-header-size --max-old-space-size --max-old-space-size-percentage --max-semi-space-size --napi-modules --network-family-autoselection --network-family-autoselection-attempt-timeout --no-addons --no-allow-addons --no-allow-child-process --no-allow-inspector --no-allow-wasi --no-allow-worker --no-async-context-frame --no-debug-arraybuffer-allocations --no-deprecation --no-disable-sigusr1 --no-disable-wasm-trap-handler --no-enable-fips --no-enable-source-maps --no-entry-url --no-experimental-addon-modules --no-experimental-detect-module --no-experimental-eventsource --no-experimental-global-navigator --no-experimental-import-meta-resolve --no-experimental-import-text --no-experimental-print-required-tla --no-experimental-repl-await --no-experimental-require-module --no-experimental-shadow-realm --no-experimental-sqlite --no-experimental-vm-modules --no-experimental-websocket --no-experimental-webstorage --no-extra-info-on-fatal-exception --no-force-async-hooks-checks --no-force-context-aware --no-force-fips --no-force-node-api-uncaught-exceptions-policy --no-frozen-intrinsics --no-global-search-paths --no-insecure-http-parser --no-network-family-autoselection --no-node-snapshot --no-openssl-legacy-provider --no-openssl-shared-config --no-pending-deprecation --no-permission --no-preserve-symlinks --no-preserve-symlinks-main --no-report-compact --no-report-exclude-env --no-report-exclude-network --no-report-on-fatalerror --no-report-on-signal --no-report-uncaught-exception --no-require-module --no-strip-types --no-test-force-exit --no-test-only --no-test-randomize --no-throw-deprecation --no-tls-max-v1.2 --no-tls-max-v1.3 --no-tls-min-v1.0 --no-tls-min-v1.1 --no-tls-min-v1.2 --no-tls-min-v1.3 --no-trace-deprecation --no-trace-env --no-trace-env-js-stack --no-trace-env-native-stack --no-trace-exit --no-trace-promises --no-trace-sigint --no-trace-sync-io --no-trace-tls --no-trace-uncaught --no-trace-warnings --no-track-heap-objects --no-use-bundled-ca --no-use-env-proxy --no-use-openssl-ca --no-use-system-ca --no-verify-base-objects --no-warnings --no-watch --no-watch-preserve-output --no-zero-fill-buffers --node-memory-debug --node-snapshot --openssl-config --openssl-legacy-provider --openssl-shared-config --pending-deprecation --perf-basic-prof --perf-basic-prof-only-functions --perf-prof --perf-prof-unwinding-info --permission --permission-audit --preserve-symlinks --preserve-symlinks-main --prof-process --redirect-warnings --report-compact --report-dir --report-directory --report-exclude-env --report-exclude-network --report-filename --report-on-fatalerror --report-on-signal --report-signal --report-uncaught-exception --require --require-module --secure-heap --secure-heap-min --snapshot-blob --stack-trace-limit --strip-types --test-coverage-branches --test-coverage-exclude --test-coverage-functions --test-coverage-include --test-coverage-include-all --test-coverage-lines --test-force-exit --test-global-setup --test-isolation --test-name-pattern --test-only --test-random-seed --test-randomize --test-reporter --test-reporter-destination --test-rerun-failures --test-shard --test-skip-pattern --throw-deprecation --title --tls-cipher-list --tls-keylog --tls-max-v1.2 --tls-max-v1.3 --tls-min-v1.0 --tls-min-v1.1 --tls-min-v1.2 --tls-min-v1.3 --trace-deprecation --trace-env --trace-env-js-stack --trace-env-native-stack --trace-event-categories --trace-event-file-pattern --trace-events-enabled --trace-exit --trace-promises --trace-require-module --trace-sigint --trace-sync-io --trace-tls --trace-uncaught --trace-warnings --track-heap-objects --unhandled-rejections --use-bundled-ca --use-env-proxy --use-largepages --use-openssl-ca --use-system-ca --v8-pool-size --verify-base-objects --warnings --watch --watch-kill-signal --watch-path --watch-preserve-output --webstorage --worker-snapshot --zero-fill-buffers -C -r".split(" ");
      const flagsWithoutLeadingDashes = new Set(
        availableFlags.map((flag) => flag.replace(/^--?/, ""))
      );
      // As in Node, the entries live in an inner set, so Set.prototype.add.call on the
      // public object changes nothing it reports.
      const inner = new Set(availableFlags);
      class NodeEnvironmentFlagsSet extends Set {
        add() { return this; }
        delete() { return false; }
        clear() {}
        has(flag) {
          if (typeof flag !== "string") return false;
          const normalized = flag.replaceAll("_", "-");
          if (normalized.startsWith("-")) {
            return inner.has(normalized.replace(/=.*$/, ""));
          }
          return flagsWithoutLeadingDashes.has(normalized);
        }
        entries() { return inner.entries(); }
        forEach(callback, thisArg = undefined) {
          inner.forEach((value) => Reflect.apply(callback, thisArg, [value, value, this]));
        }
        get size() { return inner.size; }
        values() { return inner.values(); }
      }
      NodeEnvironmentFlagsSet.prototype.keys =
        NodeEnvironmentFlagsSet.prototype[Symbol.iterator] =
          NodeEnvironmentFlagsSet.prototype.values;
      Object.freeze(NodeEnvironmentFlagsSet.prototype.constructor);
      Object.freeze(NodeEnvironmentFlagsSet.prototype);
      return Object.freeze(new NodeEnvironmentFlagsSet());
    };
    if (!(globalThis.process.allowedNodeEnvironmentFlags instanceof Set)) {
      Object.defineProperty(globalThis.process, "allowedNodeEnvironmentFlags", {
        get() {
          globalThis.process.allowedNodeEnvironmentFlags = buildAllowedFlags();
          return globalThis.process.allowedNodeEnvironmentFlags;
        },
        set(value) {
          Object.defineProperty(this, "allowedNodeEnvironmentFlags", {
            value,
            configurable: true,
            enumerable: true,
            writable: true,
          });
        },
        enumerable: true,
        configurable: true,
      });
    }
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
    globalThis.process.finalization ||= {
      register: () => undefined,
      unregister: () => undefined,
      registerBeforeExit: () => undefined
    };
    const __quenchPermissionArgs = globalThis.process.execArgv || [];
    const __quenchPermissionEnabled = __quenchPermissionArgs.includes("--permission") ||
      __quenchPermissionArgs.includes("--permission-audit");
    if (__quenchPermissionEnabled) globalThis.process.permission ||= (() => {
      const args = globalThis.process.execArgv || [];
      const enabled = args.includes("--permission");
      const audit = args.includes("--permission-audit");
      const known = new Set([
        "fs.read", "fs.write", "child", "worker", "net", "wasi",
        "addon", "inspector", "openssl.store", "ffi"
      ]);
      const grants = new Map();
      // Permission checks must not trust mutable methods on the public `path`
      // module: user code can replace path.resolve before probing a resource.
      const permissionPath = globalThis.process.getBuiltinModule("path");
      const resolvePermissionPath = permissionPath.resolve.bind(permissionPath);
      const permissionPathSeparator = permissionPath.sep;
      const revokedScopes = new Set();
      let publishing = false;
      const permissionNames = {
        "fs.read": "FileSystemRead", "fs.write": "FileSystemWrite",
        child: "ChildProcess", worker: "WorkerThreads", net: "Net",
        wasi: "WASI", addon: "Addon", inspector: "Inspector",
        "openssl.store": "OpenSSLStore", ffi: "FFI"
      };
      const permissionChannels = {
        "fs.read": "fs", "fs.write": "fs", child: "child", worker: "worker",
        net: "net", wasi: "wasi", addon: "addon", inspector: "inspector",
        "openssl.store": "openssl-store", ffi: "ffi"
      };
      for (let index = 0; index < args.length; index++) {
        const arg = args[index];
        for (const [flag, scope] of [
          ["--allow-fs-read", "fs.read"],
          ["--allow-fs-write", "fs.write"]
        ]) {
          const inline = `${flag}=`;
          const path = arg.startsWith(inline) ? arg.slice(inline.length)
            : arg === flag ? args[index + 1] : undefined;
          if (path !== undefined) {
            const paths = grants.get(scope) || [];
            paths.push(path);
            grants.set(scope, paths);
          }
        }
      }
      const booleanFlags = [
        ["--allow-child-process", "child"], ["--allow-worker", "worker"],
        ["--allow-net", "net"], ["--allow-wasi", "wasi"],
        ["--allow-addons", "addon"], ["--allow-inspector", "inspector"],
        ["--allow-openssl-store", "openssl.store"], ["--allow-ffi", "ffi"]
      ];
      for (const [flag, scope] of booleanFlags) {
        if (args.includes(flag)) grants.set(scope, true);
      }
      function invalidType(name, value) {
        const received = value === null ? "Received null" : value === undefined
          ? "Received undefined" : typeof value === "object"
            ? `Received an instance of ${Array.isArray(value) ? "Array" : value.constructor?.name || "Object"}`
            : `Received type ${typeof value} (${String(value)})`;
        throw Object.assign(new TypeError(`The "${name}" argument must be of type string. ${received}`),
          { code: "ERR_INVALID_ARG_TYPE" });
      }
      function publishDenied(scope, resource) {
        if (publishing) return;
        publishing = true;
        try {
          globalThis.process.getBuiltinModule("diagnostics_channel")
            .channel(`node:permission-model:${permissionChannels[scope]}`)
            .publish({ permission: permissionNames[scope], resource });
        } catch {} finally {
          publishing = false;
        }
      }
      function has(scope, reference) {
        if (typeof scope !== "string") invalidType("scope", scope);
        let target;
        if (reference !== undefined) {
          if (typeof reference === "string") target = reference;
          else if (reference instanceof URL) {
            if (scope.startsWith("fs") && reference.protocol !== "file:") {
              throw Object.assign(new TypeError("The URL must use the file: scheme"),
                { code: "ERR_INVALID_URL_SCHEME" });
            }
            if (scope.startsWith("fs")) target = globalThis.process.getBuiltinModule("url").fileURLToPath(reference);
          } else if (ArrayBuffer.isView(reference)) {
            if (scope.startsWith("fs")) target = new TextDecoder().decode(reference);
          } else invalidType("reference", reference);
        }
        if (!known.has(scope)) return false;
        const resource = target;
        // Audit mode reports denied checks without enforcing them, but it
        // does not turn every permission query into a grant. Keep `has()` in
        // sync with the allow-list while guards decide whether audit denials
        // should throw.
        let granted = !enabled && !audit;
        if (!granted && !revokedScopes.has(scope)) {
          const grant = grants.get(scope);
          granted = grant === true;
          if (!granted && (scope === "fs.read" || scope === "fs.write")) {
            if (reference === undefined) {
              granted = grant?.some((path) => path === "*") || false;
            } else {
              target = resolvePermissionPath(target);
              if (scope === "fs.read" && typeof globalThis.process.argv?.[1] === "string" &&
                  target === resolvePermissionPath(globalThis.process.argv[1])) {
                granted = true;
              } else {
                const matchesPath = (entry) => {
                  if (entry === "*") return true;
                  const base = resolvePermissionPath(entry);
                  if (base.includes("*")) {
                    const prefix = base.slice(0, base.indexOf("*"));
                    const directory = prefix.replace(/[\\/]+$/, "");
                    return target === directory || target.startsWith(prefix);
                  }
                  return target === base || target.startsWith(base.endsWith(permissionPathSeparator) ? base : base + permissionPathSeparator);
                };
                granted = grant?.some(matchesPath) || false;
              }
            }
          }
        }
        if (!granted) publishDenied(scope, resource);
        return granted;
      }
      function drop(scope, reference) {
        if (typeof scope !== "string") invalidType("scope", scope);
        if (reference !== undefined) {
          if (typeof reference !== "string") {
            if (reference instanceof URL && scope.startsWith("fs")) {
              if (reference.protocol !== "file:") {
                throw Object.assign(new TypeError("The URL must use the file: scheme"),
                  { code: "ERR_INVALID_URL_SCHEME" });
              }
              reference = globalThis.process.getBuiltinModule("url").fileURLToPath(reference);
            } else if (ArrayBuffer.isView(reference) && scope.startsWith("fs")) {
              reference = new TextDecoder().decode(reference);
            } else invalidType("reference", reference);
          }
        }
        if (!enabled && !audit) {
          throw Object.assign(new Error("Cannot drop permissions when permission is not enabled"),
            { code: "ERR_ACCESS_DENIED" });
        }
        const emitDrop = (permission) => {
          try {
            globalThis.process.getBuiltinModule("diagnostics_channel")
              .channel(`node:permission-model:${permission === "ChildProcess" ? "child" : "fs"}`)
              .publish({ permission, drop: true });
          } catch {}
        };
        if (scope === "fs") {
          if (reference === undefined) {
            for (const filesystemScope of ["fs.read", "fs.write"]) {
              grants.delete(filesystemScope);
              revokedScopes.add(filesystemScope);
            }
            emitDrop("FileSystemRead");
            emitDrop("FileSystemWrite");
          }
          return;
        }
        if (!known.has(scope)) {
          return;
        }
        if (scope !== "fs.read" && scope !== "fs.write" || reference === undefined) {
          grants.delete(scope);
          revokedScopes.add(scope);
          if (scope === "fs.read") emitDrop("FileSystemRead");
          else if (scope === "fs.write") emitDrop("FileSystemWrite");
          else if (scope === "child") emitDrop("ChildProcess");
          return;
        }
        const path = globalThis.process.getBuiltinModule("path");
        const target = path.resolve(reference);
        const paths = grants.get(scope);
        if (Array.isArray(paths)) {
          const remaining = paths.filter((entry) => {
            if (entry === "*") return true;
            const base = path.resolve(entry);
            // Node drops only an exact explicit grant. Ancestor grants make
            // dropping a child a no-op, while a separate child grant survives
            // dropping its ancestor.
            return base !== target;
          });
          if (remaining.length) grants.set(scope, remaining);
          else grants.delete(scope);
        }
        if (scope === "fs.read") emitDrop("FileSystemRead");
        else emitDrop("FileSystemWrite");
      }
      return Object.freeze({ has, drop });
    })();
    else delete globalThis.process.permission;
    if (typeof globalThis.process.chdir === "function" && !globalThis.process.chdir.__quenchPermissionGuard) {
      const originalChdir = globalThis.process.chdir;
      const guardedChdir = function(path) {
        if (typeof path !== "string") {
          const error = new TypeError('The "directory" argument must be of type string');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        const permission = globalThis.process.permission;
        const audit = (globalThis.process.execArgv || []).includes("--permission-audit");
        if (permission && !permission.has("fs.read", path) && !audit) {
          throw Object.assign(new Error("Access to this API has been restricted"), {
            code: "ERR_ACCESS_DENIED", permission: "FileSystemRead", resource: path,
          });
        }
        return Reflect.apply(originalChdir, this, [path]);
      };
      if (originalChdir.disabled === true) guardedChdir.disabled = true;
      guardedChdir.__quenchPermissionGuard = true;
      globalThis.process.chdir = guardedChdir;
    }
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
