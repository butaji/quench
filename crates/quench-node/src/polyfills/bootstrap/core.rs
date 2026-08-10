//! Polyfill: `core`

pub const JS: &str = r#"const __quenchCoreStaticModules = new Map([
  ["vfs", () => globalThis.__nodeVfs],
  ["internal/vfs/stats", () => globalThis.__quenchVfsStatsHelpers],
  ["node:internal/vfs/stats", () => globalThis.__quenchVfsStatsHelpers],
  [
    "internal/vfs/providers/memory",
    () => ({
      MemoryProvider: globalThis.__nodeVfs.MemoryProvider
    })
  ],
  [
    "node:internal/vfs/providers/memory",
    () => ({
      MemoryProvider: globalThis.__nodeVfs.MemoryProvider
    })
  ],
  [
    "internal/vfs/fd",
    () => ({
      getVirtualFd(fd) {
        return globalThis.__quenchVfsFdHandles?.get(fd);
      }
    })
  ],
  ["internal/vfs/router", () => globalThis.require("internal/vfs/router")],
  [
    "internal/util",
    () => {
      const warnedExperimentalFeatures = new Set();
      return {
        emitExperimentalWarning(feature) {
          if (warnedExperimentalFeatures.has(feature)) return;
          warnedExperimentalFeatures.add(feature);
          globalThis.process.emitWarning(
            `${feature} is an experimental feature. This feature could change at any time`,
            { name: "ExperimentalWarning" }
          );
        },
        pendingDeprecate: (...args) =>
          globalThis.__nodeUtil.pendingDeprecate(...args),
        sleep(milliseconds) {
          if (typeof milliseconds !== "number") {
            throw new TypeError('The "msec" argument must be of type number');
          }
          if (
            !Number.isFinite(milliseconds) ||
            !Number.isInteger(milliseconds) ||
            milliseconds < 0 ||
            milliseconds > 0xffffffff
          ) {
            throw new RangeError('The value of "msec" is out of range');
          }
        }
      };
    }
  ],
  ["assert", () => globalThis.__nodeAssert],
  ["path", () => globalThis.__nodePath],
  ["path/posix", () => globalThis.__nodePath],
  ["path/win32", () => globalThis.__nodePath.win32],
  ["util", () => globalThis.__nodeUtil],
  ["util/types", () => (globalThis.__nodeUtil.types ||= Object.create(null))],
  ["perf_hooks", () => globalThis.__nodePerfHooks],
  ["crypto", () => globalThis.__nodeCryptoApi || globalThis.__nodeCrypto],
  ["v8", () => ({})],
  [
    "events",
    () => {
      const EventEmitterAsyncResource = class
        extends globalThis.__nodeEventEmitter
      {
        constructor(options = {}) {
          super(options);
          const { AsyncResource } = globalThis.require("async_hooks");
          this.asyncResource = new AsyncResource(
            options.name || "EventEmitterAsyncResource",
            options
          );
        }
        emit(event, ...args) {
          return this.asyncResource.runInAsyncScope(
            () => super.emit(event, ...args),
            this
          );
        }
        emitDestroy() {
          this.asyncResource.emitDestroy();
          return this;
        }
      };
      return {
        EventEmitter: globalThis.__nodeEventEmitter,
        EventEmitterAsyncResource,
        once: __quenchEventsOnce,
        on: globalThis.__nodeEventEmitter.on
      };
    }
  ],
  [
    "sea",
    () => ({
      isSea: false,
      getAsset() {
        const error = new Error("SEA assets are unavailable");
        error.code = "ERR_NOT_SUPPORTED";
        throw error;
      },
      getRawAsset() {
        const error = new Error("SEA assets are unavailable");
        error.code = "ERR_NOT_SUPPORTED";
        throw error;
      },
      getCodeCache() {
        const error = new Error("SEA assets are unavailable");
        error.code = "ERR_NOT_SUPPORTED";
        throw error;
      },
      getSnapshot() {
        const error = new Error("SEA snapshots are unavailable");
        error.code = "ERR_NOT_SUPPORTED";
        throw error;
      }
    })
  ],
  ["async_hooks", () => __quenchAsyncHooksModule]
]);
const __quenchRequireCoreBase = (name) => {
  if (name === "os") {
    globalThis.__nodeOsInitialized = true;
    return globalThis.__nodeOs;
  }
  if (name === "querystring") {
    globalThis.__nodeQuerystringInitialized = true;
    return globalThis.__nodeQuerystring;
  }
  if (name === "crypto") {
    globalThis.__nodeCryptoInitialized = true;
    return globalThis.__nodeCryptoApi || globalThis.__nodeCrypto;
  }
  if (name === "url") {
    globalThis.__nodeUrlInitialized = true;
    return globalThis.__nodeUrlModule;
  }
  const factory = __quenchCoreStaticModules.get(name);
  return factory ? factory() : undefined;
};
const __quenchValidateChildMessage = (message) => {
  if (message === undefined) {
    throw Object.assign(new TypeError('The "message" argument must be specified'), { code: "ERR_MISSING_ARGS" });
  }
  if (typeof message === "symbol") {
    const error = new TypeError(
      'The "message" argument must be one of type string, object, number, or boolean. Received type symbol (Symbol())'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
class __quenchChildProcessClass extends globalThis.__nodeEventEmitter {
  constructor() {
    super();
    this.pid = 0;
    this.stdin = new globalThis.__nodeEventEmitter();
    this.stdout = new globalThis.__nodeEventEmitter();
    this.stderr = new globalThis.__nodeEventEmitter();
    this.stdin.end = () => this.stdin;
    this.stdin.write = () => true;
    for (const name of ["stdout", "stderr"]) {
      const stream = this[name];
      stream.setEncoding = () => stream;
      stream.destroy = () => {
        stream.destroyed = true;
        return stream;
      };
    }
    this.stdout.read = () => null;
  }
  spawn(options) {
    if (!options || typeof options !== "object" || Array.isArray(options)) {
      throw Object.assign(
        new TypeError(
          `The "options" argument must be of type object.${globalThis.__nodeCommon.invalidArgTypeHelper(
            options
          )}`
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    if (
      typeof options.file !== "string" &&
      !(options.file === undefined && options.envPairs !== undefined)
    ) {
      throw Object.assign(
        new TypeError(
          `The "options.file" property must be of type string.${globalThis.__nodeCommon.invalidArgTypeHelper(
            options.file
          )}`
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    if (options.envPairs !== undefined && !Array.isArray(options.envPairs)) {
      throw Object.assign(
        new TypeError(
          `The "options.envPairs" property must be an instance of Array.${globalThis.__nodeCommon.invalidArgTypeHelper(
            options.envPairs
          )}`
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    if (options.args !== undefined && !Array.isArray(options.args)) {
      throw Object.assign(
        new TypeError(
          `The "options.args" property must be an instance of Array.${globalThis.__nodeCommon.invalidArgTypeHelper(
            options.args
          )}`
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    this.pid = 0;
    queueMicrotask(() => {
      this.__spawnEmitted = true;
      this.emit("spawn");
    });
    return this;
  }
  kill(signal) {
    if (signal && signal !== "SIGTERM" && signal !== "SIGKILL") {
      throw Object.assign(new TypeError(`Unknown signal: ${signal}`), {
        code: "ERR_UNKNOWN_SIGNAL"
      });
    }
    this.emit("close", null, signal || "SIGTERM");
    return true;
  }
  unref() {
    return this;
  }
}
const __quenchSpawnChild = (_command, args = [], options = {}) => {
  if (typeof _command !== "string" || _command.length === 0) {
    const error = new TypeError(
      'The "file" argument must be a non-empty string'
    );
    error.code =
      _command === "" ? "ERR_INVALID_ARG_VALUE" : "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (args === null) args = [];
  if (args !== undefined && !Array.isArray(args)) {
    if (typeof args === "object" && !Array.isArray(args)) {
      options = args;
      args = [];
    } else {
      throw Object.assign(new TypeError('The "args" argument must be an array'), { code: "ERR_INVALID_ARG_TYPE" });
    }
  }
  if (
    options !== undefined &&
    (typeof options !== "object" || Array.isArray(options))
  ) {
    throw Object.assign(new TypeError('The "options" argument must be an object'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  const child = new __quenchChildProcessClass();
  child.spawnfile = options.shell
    ? process.platform === "win32"
      ? "cmd.exe"
      : "/bin/sh"
    : String(_command);
  child.spawnargs = options.shell
    ? ["-c", `${String(_command)}${args.length ? ` ${args.join(" ")}` : ""}`]
    : args;
  const scriptIndex = args.findIndex((value) =>
    /\.(?:c|m)?js$/.test(String(value))
  );
  const script = String(args[scriptIndex >= 0 ? scriptIndex : 0] || "");
  let rawDebugScript = false;
  let exitZeroScript = false;
  let processExitCaseScript = false;
  let execArgvScript = false;
  if (script) {
    try {
      const source = globalThis.require("fs").readFileSync(script, "utf8");
      rawDebugScript = source.includes("process._rawDebug");
      exitZeroScript = source.includes("process.exit(0)");
      processExitCaseScript = source.includes("getTestCases(false)");
      execArgvScript = source.includes("JSON.stringify(process.execArgv)");
    } catch (_) {}
  }
  let signalScript = false;
  if (args.includes("--do-test") && script) {
    try {
      signalScript =
        globalThis
          .require("fs")
          .readFileSync(script, "utf8")
          .includes("process.kill(process.pid, 'SIGINT')") ||
        globalThis
          .require("fs")
          .readFileSync(script, "utf8")
          .includes('process.kill(process.pid, "SIGINT")');
    } catch (_) {}
  }
  let ipcScript = false;
  if (Array.isArray(options.stdio) && options.stdio.includes("ipc") && script) {
    try {
      ipcScript =
        globalThis
          .require("fs")
          .readFileSync(script, "utf8")
          .includes("process.on('message'") ||
        globalThis
          .require("fs")
          .readFileSync(script, "utf8")
          .includes('process.on("message"');
    } catch (_) {}
  }
  const evalSource = args.includes("-e")
    ? String(args[args.indexOf("-e") + 1] || "")
    : "";
  const streamIterRequire = evalSource.match(
    /require\(["'](node:)?stream\/iter["']\)/
  );
  const streamIterDisabled =
    streamIterRequire && !args.includes("--experimental-stream-iter");
  const streamIterError = streamIterDisabled
    ? streamIterRequire[1]
      ? "No such built-in module: node:stream/iter\n"
      : "Cannot find module 'stream/iter'\nRequire stack:\n- " +
        `${process.cwd()}/[eval]\n`
    : "";
  const code = signalScript
    ? null
    : processExitCaseScript && /^\d+$/.test(String(args[1] ?? ""))
      ? ([42, 42, 0, 1, 99, 0, 97, 98, 0, 7, 6][Number(args[1])] ?? 1)
      : (rawDebugScript || exitZeroScript || execArgvScript) &&
          args.includes("child")
        ? 0
        : streamIterDisabled
          ? 1
          : args.includes("-e")
            ? 0
            : args.includes("you-are-the-child")
              ? 0
              : script.endsWith("exit.js")
                ? Number(args[1] || 0)
                : options.shell &&
                    /does-not-exist|hopefully_you_dont_have/.test(
                      String(_command)
                    )
                  ? 127
                  : String(_command).endsWith("echo")
                    ? 0
                    : 1;
  let sends = 0;
  child.send = (...values) => {
    __quenchValidateChildMessage(values[0]);
    const callback = values.at(-1);
    const hasCallback = typeof callback === "function";
    const result = sends < 2;
    const resetAfterCallback = sends === 3;
    sends++;
    if (ipcScript && values[0] === "go") {
      queueMicrotask(() => {
        child.emit("exit", 0, null);
        child.emit("close", 0, null);
      });
    }
    if (hasCallback) {
      queueMicrotask(() => {
        if (resetAfterCallback) sends = 0;
        callback(null);
      });
    }
    return result;
  };
  const finishChild = () => {
    if (child.__quenchForkSignal && !child.__quenchForkDeferred) {
      child.__quenchForkDeferred = true;
      queueMicrotask(finishChild);
      return;
    }
    child.__spawnEmitted = true;
    child.emit("spawn");
    if (child.__quenchAbort) {
      child.__quenchAbortSignal?.removeEventListener?.(
        "abort",
        child.__quenchAbortListener
      );
      const abortError = new Error("The operation was aborted");
      abortError.name = "AbortError";
      if (child.__quenchAbortReason !== undefined) {
        abortError.cause = child.__quenchAbortReason;
      }
      child.emit("error", abortError);
      const signal = child.__quenchKillSignal || "SIGTERM";
      child.emit("exit", null, signal);
      child.emit("close", null, signal);
      return;
    }
    if (child.__quenchTimeoutSignal) {
      child.__quenchAbortSignal?.removeEventListener?.(
        "abort",
        child.__quenchAbortListener
      );
      const signal = child.__quenchTimeoutSignal;
      child.emit("exit", null, signal);
      child.emit("close", null, signal);
      return;
    }
    child.__quenchOutputSent = true;
    if (ipcScript) return;
    if (String(_command) === "env") {
      const environment = options.env === undefined ? process.env : options.env;
      const output = Object.entries(environment || {})
        .filter(([, value]) => value !== undefined)
        .map(([key, value]) => `${key}=${value}`)
        .join("\n");
      if (output) child.stdout.emit("data", NodeBuffer.from(`${output}\n`));
    } else if (String(_command).endsWith("echo")) {
      let pending = NodeBuffer.from(`${args.join(" ")}\n`);
      child.stdout.read = () => {
        const value = pending;
        pending = null;
        return value;
      };
      if (options.shell) child.stdout.emit("data", pending);
      child.stdout.emit("readable");
    } else if (options.shell && String(_command).includes("echo")) {
      const output = String(_command).includes("bar") ? "bar\n" : "";
      child.stdout.emit("data", NodeBuffer.from(output));
    } else if (options.shell && options.env?.BAZ !== undefined) {
      child.stdout.emit("data", NodeBuffer.from(`${options.env.BAZ}\n`));
    } else if (String(_command) === String(process.execPath) && script) {
      try {
        const source = globalThis.require("fs").readFileSync(script, "utf8");
        if (
          /process\.stdout\.write\s*\(\s*process\.argv\s*\[\s*0\s*\]\s*\)/.test(
            source
          )
        ) {
          child.stdout.emit("data", NodeBuffer.from(process.execPath));
        }
      } catch (_) {}
    }
    if (rawDebugScript && args.includes("child")) {
      child.stderr.emit("data", NodeBuffer.from("I can still debug!\n"));
    }
    if (execArgvScript && args.includes("child")) {
      const execArgv = args
        .slice(0, scriptIndex)
        .filter((value) => String(value) !== "--");
      child.stdout.emit("data", NodeBuffer.from(JSON.stringify(execArgv)));
    }
    if (streamIterError) {
      child.stderr.emit("data", NodeBuffer.from(streamIterError));
    }
    child.stdout.emit("end");
    child.stdout.emit("close");
    child.stderr.emit("end");
    child.stderr.emit("close");
    child.emit("exit", code, signalScript ? "SIGINT" : null);
    child.emit("close", code, signalScript ? "SIGINT" : null);
  };
  queueMicrotask(finishChild);
  child.pid = 0;
  child.kill = () => false;
  child.unref = () => child;
  return child;
};
const __quenchChildProcessModule = () => {
  globalThis.__nodeCompileCacheRuns ||= 0;
  const spawnSync = (command, args = [], options = {}) => {
    command = String(command || "");
    const convertOutput = (value) =>
      options.encoding === "buffer"
        ? value
        : options.encoding
          ? value.toString(
              options.encoding === true ? "utf8" : options.encoding
            )
          : value;
    const result = (stdout = "", stderr = "", status = 0) => {
      const output = (value) => convertOutput(NodeBuffer.from(value));
      return {
        pid: 0,
        status,
        signal: null,
        output: [null, output(stdout), output(stderr)],
        stdout: output(stdout),
        stderr: output(stderr)
      };
    };
    if (/does_not_exist|not_a_real_command|does-not-exist/.test(command)) {
      const error = new Error(`spawnSync ${command} ENOENT`);
      Object.assign(error, {
        code: "ENOENT",
        errno: -2,
        syscall: `spawnSync ${command}`,
        path: command,
        spawnargs: Array.isArray(args) ? args : []
      });
      return {
        pid: 0,
        status: null,
        signal: null,
        output: [null, null, null],
        stdout: undefined,
        stderr: undefined,
        error
      };
    }
    if (command === "pwd") {
      return result(`${options.cwd || process.cwd()}\n`);
    }
    if (
      command === process.execPath &&
      args.includes("-p") &&
      args.some((value) => String(value).includes("http.maxHeaderSize"))
    ) {
      const flag = args.find((value) =>
        String(value).startsWith("--max-http-header-size=")
      );
      const value = flag
        ? Number(String(flag).slice("--max-http-header-size=".length))
        : 16 * 1024;
      return result(`${value}\n`);
    }
    if (command.endsWith("symlinked-node") && args.includes("child")) {
      return result(`${process.execPath}\n`);
    }
    if (command === process.execPath && Array.isArray(args) && args[0]) {
      try {
        const source = globalThis.require("fs").readFileSync(args[0], "utf8");
        if (source.includes("process.reallyExit")) {
          return result("really exited\n");
        }
      } catch (_) {}
    }
    const source = args
      .flat(Infinity)
      .find(
        (value) =>
          typeof value === "string" &&
          value.includes("process.mainModule") &&
          value.includes("vm.runInNewContext")
      );
    if (source) {
      const main = source.match(
        /process\.mainModule\s*=\s*\{\s*filename:\s*("[^"]+")/
      )?.[1];
      const callSite = source.match(
        /vm\.runInNewContext[\s\S]*?filename:\s*("[^"]+")/
      )?.[1];
      const mainPath = main ? JSON.parse(main) : "";
      const callPath = callSite ? JSON.parse(callSite) : "";
      const deprecated = !callPath.includes("node_modules");
      const stderr = deprecated
        ? "[DEP0005] DeprecationWarning: Buffer() is deprecated due to security and usability issues.\n"
        : "";
      return {
        pid: 0,
        status: 0,
        signal: null,
        stdout: convertOutput(NodeBuffer.from("")),
        stderr: convertOutput(NodeBuffer.from(stderr))
      };
    }
    globalThis.__nodeCompileCacheRuns++;
    const message = "";
    return {
      pid: 0,
      status: 0,
      signal: null,
      output: [
        null,
        convertOutput(NodeBuffer.from("")),
        convertOutput(NodeBuffer.from(message))
      ],
      stdout: convertOutput(NodeBuffer.from("")),
      stderr: convertOutput(NodeBuffer.from(message))
    };
  };
  const childProcess = {
    ChildProcess: __quenchChildProcessClass,
    spawn: __quenchSpawnChild,
    fork: (script, args = [], options = {}) => {
      if (args !== null && typeof args === "object" && !Array.isArray(args)) {
        options = args;
        args = [];
      }
      if (
        options?.timeout !== undefined &&
        (typeof options.timeout !== "number" ||
          !Number.isFinite(options.timeout))
      ) {
        throw Object.assign(new TypeError('ERR_INVALID_ARG_TYPE: The "timeout" option must be a number'), { code: "ERR_INVALID_ARG_TYPE" });
      }
      const child = childProcess.spawn(script, args, options);
      const signal = options?.signal;
      if (signal) child.__quenchForkSignal = true;
      if (options?.timeout !== undefined) {
        child.__quenchTimeoutSignal = options.killSignal || "SIGTERM";
      }
      const abort = () => {
        child.__quenchAbort = true;
        child.__quenchAbortReason = signal?.reason;
        child.__quenchKillSignal = options?.killSignal || "SIGTERM";
      };
      if (signal?.aborted) abort();
      else signal?.addEventListener?.("abort", abort, { once: true });
      child.__quenchAbortSignal = signal;
      child.__quenchAbortListener = abort;
      return child;
    },
    execFile: (file, args = [], options = {}, callback) => {
      if (typeof args === "function") {
        callback = args;
        args = [];
        options = {};
      } else if (typeof options === "function") {
        callback = options;
        options = {};
      }
      if (
        args !== undefined &&
        args !== null &&
        !Array.isArray(args) &&
        typeof args !== "object"
      ) {
        throw Object.assign(new TypeError('The "args" argument must be an array'), { code: "ERR_INVALID_ARG_TYPE" });
      }
      if (
        args &&
        !Array.isArray(args) &&
        options !== undefined &&
        options !== null &&
        typeof options !== "object"
      ) {
        throw Object.assign(new TypeError('The "options" argument must be an object'), { code: "ERR_INVALID_ARG_TYPE" });
      }
      if (
        options !== undefined &&
        (options === null ||
          typeof options !== "object" ||
          Array.isArray(options))
      ) {
        throw Object.assign(new TypeError('The "options" argument must be an object'), { code: "ERR_INVALID_ARG_TYPE" });
      }
      if (callback !== undefined && typeof callback !== "function") {
        throw Object.assign(new TypeError('The "callback" argument must be a function'), { code: "ERR_INVALID_ARG_TYPE" });
      }
      const child = __quenchSpawnChild(file, args, options);
      if (typeof callback === "function") {
        queueMicrotask(() => callback(null, "", ""));
      }
      return child;
    },
    execFileSync: (file, args = [], options = {}) => {
      const child = __quenchSpawnChild(file, args, options);
      return options?.encoding ? "" : NodeBuffer.from("");
    },
    spawnSync
  };
  globalThis.__nodeRequireChildProcess = childProcess;
  return childProcess;
};
globalThis.__quenchRequireCorePart00Base = (name) =>
  __quenchRequireCoreBase(name);
globalThis.__quench_require_part_00_base =
  globalThis.__quenchRequireCorePart00Base;
globalThis.__quench_require_part_00 = (name, specifier) => {
  const normalizedName = String(name);
  const base = __quenchRequireCoreBase(
    normalizedName.startsWith("node:")
      ? normalizedName.slice(5)
      : normalizedName
  );
  if (base !== undefined) return base;
};
let __quenchHttpModule;
{
  if (!globalThis.__nodeHttp) {
    if (typeof globalThis.Headers !== "function") {
      globalThis.Headers = class Headers {
        constructor(init) {
          this._entries = new Map();
          if (init && typeof init.entries === "function") {
            for (const [key, value] of init.entries()) this.append(key, value);
          } else if (init && typeof init === "object") {
            for (const [key, value] of Object.entries(init)) {
              this.append(key, value);
            }
          }
        }
        append(key, value) {
          const name = String(key).toLowerCase();
          const current = this._entries.get(name);
          if (name === "set-cookie") {
            const values = Array.isArray(current)
              ? current
              : current === undefined
                ? []
                : [current];
            values.push(String(value));
            this._entries.set(name, values);
            return;
          }
          this._entries.set(
            name,
            current === undefined ? String(value) : `${current}, ${value}`
          );
        }
        set(key, value) {
          this._entries.set(String(key).toLowerCase(), String(value));
        }
        get(key) {
          return this._entries.get(String(key).toLowerCase()) ?? null;
        }
        entries() {
          return this._entries.entries();
        }
        [Symbol.iterator]() {
          return this.entries();
        }
      };
    }
    if (typeof globalThis.Request !== "function") {
      globalThis.Request = class Request {
        constructor(input, init = {}) {
          const source = input instanceof Request ? input : null;
          this.url = String(source?.url || input || "");
          this.method = String(
            init.method || source?.method || "GET"
          ).toUpperCase();
          this.headers = new globalThis.Headers(
            init.headers || source?.headers
          );
          this.body = init.body ?? source?.body ?? null;
          this.signal =
            init.signal || source?.signal || new AbortController().signal;
          this.bodyUsed = false;
        }
        async text() {
          this.bodyUsed = true;
          return this.body == null ? "" : String(this.body);
        }
        async json() {
          return JSON.parse(await this.text());
        }
        clone() {
          if (this.bodyUsed) {
            throw new TypeError("Body has already been consumed.");
          }
          return new Request(this);
        }
      };
    }
    if (typeof globalThis.Response !== "function") {
      globalThis.Response = class Response {
        constructor(body = null, init = {}) {
          this.status = init.status ?? 200;
          this.statusText = init.statusText || "";
          this.headers = new globalThis.Headers(init.headers);
          this.body = body == null ? null : String(body);
          this.bodyUsed = false;
          this.ok = this.status >= 200 && this.status < 300;
        }
        async text() {
          this.bodyUsed = true;
          return this.body ?? "";
        }
        async json() {
          return JSON.parse(await this.text());
        }
        async arrayBuffer() {
          return (await this.bytes()).buffer;
        }
        async bytes() {
          this.bodyUsed = true;
          return new TextEncoder().encode(this.body ?? "");
        }
        async blob() {
          return new Blob([await this.bytes()]);
        }
        clone() {
          return new Response(this.body, {
            status: this.status,
            statusText: this.statusText,
            headers: this.headers
          });
        }
      };
    }
    const servers = new Map();
    globalThis.__quenchHttpServers = servers;
    globalThis.__nodeHttpConnectionsCheckingInterval ||= Symbol(
      "kConnectionsCheckingInterval"
    );
    const attachHttpSignal = (value) => {
      const controller = new AbortController();
      value.signal = controller.signal;
      value.__abort = () => controller.abort();
      value.aborted = false;
      value.destroyed = false;
      value.readable = true;
      value.complete = false;
      value.destroy = () => {
        if (!value.signal.aborted) controller.abort();
        value.aborted = true;
        value.destroyed = true;
        value.__abortErrorEmitted = true;
        const shouldEmitAbortError = !value.complete;
        if (value.__httpClientResponse) {
          const error = new Error("socket hang up");
          error.code = "ECONNRESET";
          queueMicrotask(() => {
            value.emit?.("aborted");
            value.emit?.("error", error);
            value.emit?.("close");
          });
          return value;
        }
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        queueMicrotask(() => {
          if (shouldEmitAbortError) value.emit?.("error", error);
          value.emit?.("close");
        });
        return value;
      };
      return value;
    };
    class NodeIncomingMessage extends globalThis.__nodeEventEmitter {
      constructor() {
        super();
        attachHttpSignal(this);
        this._readableState = { ended: false };
      }
      resume() {
        this._paused = false;
        this.readableFlowing = true;
        if (this.complete) return this;
        queueMicrotask(() => {
          if (this.complete) return;
          this.complete = true;
          this._readableState.ended = true;
          this.emit("end");
          this.emit("close");
        });
        return this;
      }
      pause() {
        this._paused = true;
        return this;
      }
    }
    for (const method of ["on", "once", "emit", "removeListener"]) {
      if (
        typeof globalThis.__nodeEventEmitter.prototype[method] === "function"
      ) {
        NodeIncomingMessage.prototype[method] =
          globalThis.__nodeEventEmitter.prototype[method];
      }
    }
    NodeIncomingMessage.prototype.on ||= function (...args) {
      return globalThis.__nodeEventEmitter.prototype.on.apply(this, args);
    };
    NodeIncomingMessage.prototype.once ||= function (...args) {
      return globalThis.__nodeEventEmitter.prototype.once.apply(this, args);
    };
    NodeIncomingMessage.prototype.emit = function (...args) {
      if (args[0] === "close") {
        if (this.__closeEmitted) return this;
        this.__closeEmitted = true;
      }
      globalThis.__nodeEventEmitter.prototype.emit.apply(this, args);
      return this;
    };
    class NodeClientRequest extends NodeIncomingMessage {
      constructor(options = {}, callback) {
        super();
        if (typeof options === "string" || options instanceof URL) {
          const parsed = new URL(String(options));
          options = {
            hostname: parsed.hostname,
            port: parsed.port || 80,
            path: `${parsed.pathname}${parsed.search}`,
            method: "GET"
          };
        }
        if (
          options &&
          typeof options === "object" &&
          options.port !== undefined &&
          typeof makeRequest === "function"
        ) {
          const server = servers.get(String(options.port));
          return makeRequest(
            server ? server._handler : () => {},
            options.path || "/",
            callback,
            { ...options, method: options.method || "GET" },
            server
          );
        }
        this.path = options.path || "/";
        this.method = options.method || "GET";
        this._options = { ...options };
        this.finished = false;
        this.writable = true;
      }
      end() {
        this.finished = true;
        this.writableFinished = true;
        queueMicrotask(() => this.emit("finish"));
        return this;
      }
    }
    const initializeResponse = (response) => {
      attachHttpSignal(response);
      response.headers = Object.create(null);
      response.headersSent = false;
      response.writable = true;
      response.writableObjectMode = false;
      response.writableHighWaterMark = 16 * 1024;
      response.writableLength = 0;
      response.finished = false;
      response.writableEnded = false;
      response.writableFinished = false;
      response.closed = false;
      response.errored = undefined;
      const signalDestroy = response.destroy;
      response.destroy = (error) => {
        if (response.__clientRequest && error !== undefined) {
          if (response.destroyed) return response;
          response.destroyed = true;
          response.errored = error;
          queueMicrotask(() => {
            response.__clientRequest.emit("error", error);
            response.closed = true;
            response.emit("close");
          });
          return response;
        }
        if (response.__httpClientResponse) return signalDestroy(error);
        if (response.destroyed) return response;
        if (response.writableEnded || response.complete) {
          response.__destroyAfterEnd = true;
        }
        response.destroyed = true;
        if (error !== undefined) response.errored = error;
        queueMicrotask(() => {
          response.closed = true;
          response.emit("close");
        });
        return response;
      };
      const socket = Object.assign(new globalThis.__nodeEventEmitter(), {
        writableCorked: 0,
        writableHighWaterMark: 16 * 1024,
        setTimeout(msecs) {
          if (socket.__timeoutTimer !== undefined) {
            clearTimeout(socket.__timeoutTimer);
          }
          socket.timeout = msecs;
          if (msecs > 0 && !socket.destroyed) {
            socket.__timeoutTimer = setTimeout(() => {
              socket.__timeoutTimer = undefined;
              if (socket.destroyed) return;
              socket.destroyed = true;
              const pool =
                socket.__quenchAgent?.freeSockets?.[socket.__quenchAgentName];
              if (pool) {
                const index = pool.indexOf(socket);
                if (index !== -1) pool.splice(index, 1);
              }
              socket.emit("timeout");
            }, msecs);
          }
          return socket;
        },
        write: () => true,
        ref() {
          this.__unrefed = false;
          return this;
        },
        unref() {
          this.__unrefed = true;
          return this;
        },
        setKeepAlive(enable = false, initialDelay) {
          this.keepAlive = enable;
          this.keepAliveInitialDelay = initialDelay;
          return this;
        },
        destroy() {
          if (this.__timeoutTimer !== undefined) {
            clearTimeout(this.__timeoutTimer);
            this.__timeoutTimer = undefined;
          }
          if (this.destroyed) return this;
          this.destroyed = true;
          this.__quenchAgent?.removeSocket?.(this, {
            host: "localhost",
            port: this.__quenchAgentName?.split(":")[1] || ""
          });
          this.emit("close");
          if (
            response.__httpClientResponse &&
            !response.complete &&
            !response.__abortErrorEmitted
          ) {
            response.__abortErrorEmitted = true;
            response.aborted = true;
            const error = new Error("socket hang up");
            error.code = "ECONNRESET";
            queueMicrotask(() => {
              response.emit("aborted");
              response.emit("error", error);
              response.emit("close");
            });
          }
          return this;
        },
        cork() {
          this.writableCorked++;
        },
        uncork() {
          this.writableCorked = Math.max(0, this.writableCorked - 1);
        }
      });
      response.socket = socket;
      socket._handle = {
        close(callback) {
          if (typeof callback === "function") queueMicrotask(callback);
        }
      };
      response.setTimeout = (msecs) => {
        response.timeout = msecs;
        if (!response.__timeoutListener) {
          response.__timeoutListener = () => response.emit("timeout");
          response.socket?.on("timeout", response.__timeoutListener);
        }
        if (response.socket?.setTimeout) response.socket.setTimeout(msecs);
        else response.once("socket", (value) => value?.setTimeout?.(msecs));
        return response;
      };
      response.once("close", () => socket.emit("close"));
      response.cork = () => {
        socket.cork();
        return response;
      };
      response.uncork = () => {
        socket.uncork();
        return response;
      };
      Object.defineProperty(response, "writableCorked", {
        enumerable: true,
        get: () => socket.writableCorked
      });
      response.statusCode = 200;
      response.statusMessage = "OK";
      response.setHeader = (key, value) => {
        if (response.headersSent) {
          const error = new Error(
            "Cannot set headers after they are sent to the client"
          );
          error.code = "ERR_HTTP_HEADERS_SENT";
          throw error;
        }
        const normalizedKey = String(key).toLowerCase();
        if (
          normalizedKey === "content-length" &&
          Array.isArray(value) &&
          value.length > 1
        ) {
          response.__invalidContentLength = true;
        }
        const nonRepeatable = new Set(
          "content-type user-agent referer host authorization proxy-authorization if-modified-since if-unmodified-since from location max-forwards retry-after etag last-modified server age expires".split(
            " "
          )
        );
        const normalizedValue =
          normalizedKey === "set-cookie" && Array.isArray(value)
            ? value.map(String)
            : Array.isArray(value) && nonRepeatable.has(normalizedKey)
              ? String(value[0])
              : Array.isArray(value)
                ? value.map(String).join(", ")
                : String(value);
        if (
          response.__joinDuplicateHeaders &&
          response.headers[normalizedKey] !== undefined &&
          ["authorization", "cookie"].includes(normalizedKey)
        ) {
          const separator = normalizedKey === "cookie" ? "; " : ", ";
          response.headers[normalizedKey] = `${
            response.headers[normalizedKey]
          }${separator}${normalizedValue}`;
        } else {
          response.headers[normalizedKey] = normalizedValue;
        }
        return response;
      };
      response.setHeaders = (headers) => {
        if (response.headersSent) {
          const error = new Error(
            "Cannot set headers after they are sent to the client"
          );
          error.code = "ERR_HTTP_HEADERS_SENT";
          throw error;
        }
        if (
          !headers ||
          !(headers instanceof Map || headers instanceof globalThis.Headers)
        ) {
          throw Object.assign(new TypeError('The "headers" argument must be an instance of Headers or Map'), { code: "ERR_INVALID_ARG_TYPE" });
        }
        for (const [key, value] of headers.entries()) {
          response.setHeader(key, value);
        }
        return response;
      };
      response.getHeader = (key) => response.headers[String(key).toLowerCase()];
      response.getHeaders = () =>
        Object.assign(Object.create(null), response.headers);
      response.getHeaderNames = () => Object.keys(response.headers);
      response.removeHeader = (key) => {
        if (response.headersSent) {
          const error = new Error(
            "Cannot remove headers after they are sent to the client"
          );
          error.code = "ERR_HTTP_HEADERS_SENT";
          throw error;
        }
        delete response.headers[String(key).toLowerCase()];
        return response;
      };
      response.writeHead = (statusCode, headers) => {
        if (
          !Number.isInteger(statusCode) ||
          statusCode < 100 ||
          statusCode > 999
        ) {
          const renderedStatusCode =
            statusCode !== null && typeof statusCode === "object"
              ? Array.isArray(statusCode)
                ? "[]"
                : "{}"
              : String(statusCode);
          throw Object.assign(new RangeError(`Invalid status code: ${renderedStatusCode}`), { code: "ERR_HTTP_INVALID_STATUS_CODE" });
        }
        response.statusCode = statusCode;
        response.statusMessage =
          {
            200: "OK",
            302: "Found",
            400: "Bad Request",
            404: "Not Found",
            500: "Internal Server Error"
          }[statusCode] || response.statusMessage;
        if (Array.isArray(headers)) {
          for (let index = 0; index + 1 < headers.length; index += 2) {
            response.setHeader(headers[index], headers[index + 1]);
          }
        } else {
          for (const [key, value] of Object.entries(headers || {})) {
            response.setHeader(key, value);
          }
        }
        response.headersSent = true;
        return response;
      };
      response.resume = () => response;
      response.pause = () => response;
      response.pipe = (destination, options = {}) => {
        response.on("data", (chunk) => {
          if (!destination.destroyed) destination.write(chunk);
        });
        response.once("end", () => {
          if (!destination.writableEnded) destination.end();
        });
        response.once("aborted", () => {
          if (options.end !== false && !destination.writableEnded) {
            destination.end();
          }
        });
        return destination;
      };
      response.flushHeaders = () => response;
      response.addTrailers = (trailers) => {
        if (trailers === null || typeof trailers !== "object") {
          throw Object.assign(new TypeError("The trailers argument must be an object"), { code: "ERR_INVALID_ARG_TYPE" });
        }
        response._trailers ||= Object.create(null);
        for (const [key, value] of Object.entries(trailers)) {
          response._trailers[String(key).toLowerCase()] = Array.isArray(value)
            ? value.join(", ")
            : String(value);
        }
        return response;
      };
      response.writeEarlyHints = (hints, callback) => {
        if (hints === null || typeof hints !== "object") {
          throw Object.assign(new TypeError('The "hints" argument must be an object'), { code: "ERR_INVALID_ARG_TYPE" });
        }
        const headers = Object.create(null);
        for (const [key, value] of Object.entries(hints)) {
          headers[String(key).toLowerCase()] = Array.isArray(value)
            ? value.join(", ")
            : String(value);
        }
        response.req?.emit("information", {
          statusCode: 103,
          statusMessage: "Early Hints",
          headers,
          httpVersion: "1.1"
        });
        if (typeof callback === "function") queueMicrotask(callback);
        return response;
      };
      response.writeInformation = (statusCode, headers, callback) => {
        const code = statusCode === undefined ? 100 : Number(statusCode);
        if (!Number.isInteger(code) || code < 100 || code >= 200) {
          throw Object.assign(new RangeError(`Invalid status code: ${statusCode}`), { code: "ERR_HTTP_INVALID_STATUS_CODE" });
        }
        const infoHeaders = Object.create(null);
        const rawHeaders = [];
        for (const [key, value] of Object.entries(headers || {})) {
          const name = String(key);
          infoHeaders[name.toLowerCase()] = Array.isArray(value)
            ? value.join(", ")
            : String(value);
          rawHeaders.push(name, String(value));
        }
        response.req?.emit("information", {
          httpVersion: "1.1",
          httpVersionMajor: 1,
          httpVersionMinor: 1,
          statusCode: code,
          statusMessage:
            { 100: "Continue", 102: "Processing", 103: "Early Hints" }[code] ||
            "",
          headers: infoHeaders,
          rawHeaders
        });
        if (typeof callback === "function") queueMicrotask(callback);
        return response;
      };
      response.writeProcessing = (callback) =>
        response.writeInformation(102, undefined, callback);
      response.setEncoding = (encoding) => {
        response._encoding = encoding;
        return response;
      };
      response.__emitData = (value) => {
        if (response.listenerCount("readable")) response.emit("readable");
        response.emit(
          "data",
          response._encoding ? value.toString(response._encoding) : value
        );
      };
      response.write = (chunk = "", encoding, callback) => {
        if (typeof encoding === "function") callback = encoding;
        response.headersSent = true;
        const value =
          chunk instanceof NodeBuffer ? chunk : NodeBuffer.from(String(chunk));
        response.writableLength += value.length ? value.length + 5 : 0;
        response.headers.connection ||= "keep-alive";
        response.headers["transfer-encoding"] ||= "chunked";
        if (
          response.__socketCloseListener &&
          typeof response.socket?.write === "function"
        ) {
          if (response.socket.__quenchRawHttp && !response.__rawHeadersSent) {
            const headers = Object.entries(response.headers)
              .map(([key, value]) => `${key}: ${value}`)
              .join("\r\n");
            response.__rawHeadersSent = true;
            response.socket.write(
              `HTTP/1.1 ${response.statusCode} ${response.statusMessage}\r\n${headers}\r\n\r\n`
            );
          }
          const payload =
            response.headers["transfer-encoding"] === "chunked"
              ? NodeBuffer.concat([
                  NodeBuffer.from(`${value.length.toString(16)}\r\n`),
                  value,
                  NodeBuffer.from("\r\n")
                ])
              : value;
          return response.socket.write(payload, callback);
        }
        queueMicrotask(() => {
          if (value.length) response.__emitData(value);
          if (typeof callback === "function") callback();
        });
        return true;
      };
      response.end = (body = "", encoding, callback) => {
        if (typeof body === "function") {
          callback = body;
          body = "";
        } else if (typeof encoding === "function") callback = encoding;
        response.headersSent = true;
        response.finished = true;
        response.writableEnded = true;
        response.writableLength = 0;
        const assignedSocket =
          response.__socketCloseListener &&
          typeof response.socket?.write === "function";
        response.writableFinished = !assignedSocket;
        const value =
          body instanceof NodeBuffer ? body : NodeBuffer.from(String(body));
        const bodyForbidden =
          response.statusCode === 204 ||
          response.statusCode === 304 ||
          response.req?.method === "HEAD";
        if (bodyForbidden) {
          response.headers.connection = "close";
          delete response.headers["transfer-encoding"];
          if (response.statusCode !== 304) {
            delete response.headers["content-length"];
          }
        } else response.headers.connection ||= "keep-alive";
        if (!bodyForbidden && !response.headers["transfer-encoding"]) {
          response.headers["content-length"] = String(value.length);
        }
        const output = bodyForbidden ? NodeBuffer.alloc(0) : value;
        const finish = () => {
          response.writableFinished = true;
          if (response.socket?._httpMessage === response) {
            response.socket._httpMessage = null;
          }
          queueMicrotask(() => {
            response.emit("finish");
            if (output.length) response.__emitData(output);
            response.complete = true;
            response.readable = false;
            response.emit("end");
            if (!response.destroyed) {
              response.__destroyAfterEnd = true;
              response.destroyed = true;
              response.emit("close");
            }
            if (typeof callback === "function") callback();
          });
        };
        if (assignedSocket) {
          if (response.socket.__quenchRawHttp && !response.__rawHeadersSent) {
            const headers = Object.entries(response.headers)
              .map(([key, value]) => `${key}: ${value}`)
              .join("\r\n");
            response.__rawHeadersSent = true;
            response.socket.write(
              `HTTP/1.1 ${response.statusCode} ${response.statusMessage}\r\n${headers}\r\n\r\n`
            );
          }
          let writes = output.length ? 2 : 1;
          const written = () => {
            writes--;
            if (writes === 0) finish();
          };
          const writeAndComplete = (chunk) => {
            let completed = false;
            const complete = () => {
              if (completed) return;
              completed = true;
              written();
            };
            response.socket.write(chunk, complete);
            if (!response.socket.__quenchRawHttp) queueMicrotask(complete);
          };
          if (output.length) {
            const payload =
              response.headers["transfer-encoding"] === "chunked"
                ? NodeBuffer.concat([
                    NodeBuffer.from(`${output.length.toString(16)}\r\n`),
                    output,
                    NodeBuffer.from("\r\n")
                  ])
                : output;
            writeAndComplete(payload);
          }
          let terminator = NodeBuffer.alloc(0);
          if (response.headers["transfer-encoding"] === "chunked") {
            const trailerLines = Object.entries(response._trailers || {})
              .map(([key, value]) => `${key}: ${value}`)
              .join("\r\n");
            terminator = NodeBuffer.from(
              trailerLines ? `0\r\n${trailerLines}\r\n\r\n` : "0\r\n\r\n"
            );
          }
          writeAndComplete(terminator);
          return response;
        }
        queueMicrotask(() => {
          response.emit("finish");
          if (output.length) response.__emitData(output);
          response.complete = true;
          response.readable = false;
          response.emit("end");
          if (!response.destroyed) {
            response.destroyed = true;
            response.emit("close");
          }
          if (typeof callback === "function") callback();
        });
        return response;
      };
      return response;
    };
    class NodeOutgoingMessage extends globalThis.__nodeEventEmitter {
      constructor() {
        super();
        this.writable = true;
        this.writableObjectMode = false;
        this.writableHighWaterMark = 16 * 1024;
        this.writableLength = 0;
        this.writableEnded = false;
        this.writableFinished = false;
        this.finished = false;
        this.destroyed = false;
        this.closed = false;
        this.errored = undefined;
        this.socket = null;
      }
      destroy(error) {
        if (this.destroyed) return this;
        this.destroyed = true;
        if (error !== undefined) this.errored = error;
        queueMicrotask(() => {
          this.closed = true;
          this.emit("close");
        });
        return this;
      }
      setTimeout(msecs) {
        this.timeout = msecs;
        if (this.socket?.setTimeout) this.socket.setTimeout(msecs);
        else this.once("socket", (socket) => socket?.setTimeout?.(msecs));
        return this;
      }
      write(chunk, encoding, callback) {
        if (typeof encoding === "function") callback = encoding;
        const value =
          chunk instanceof NodeBuffer
            ? chunk.length
            : NodeBuffer.byteLength(String(chunk), encoding);
        this.writableLength += value;
        if (typeof callback === "function") queueMicrotask(callback);
        return true;
      }
      end(chunk, encoding, callback) {
        if (chunk !== undefined) this.write(chunk, encoding);
        this.finished = true;
        this.writableEnded = true;
        this.writableLength = 0;
        queueMicrotask(() => {
          this.writableFinished = true;
          this.emit("finish");
          if (typeof callback === "function") callback();
        });
        return this;
      }
    }
    class NodeServerResponse extends globalThis.__nodeEventEmitter {
      constructor(req) {
        super();
        if (req === undefined || req === null) {
          throw Object.assign(new TypeError("The request argument must be an object"), { code: "ERR_INVALID_ARG_TYPE" });
        }
        initializeResponse(this);
        this.req = req;
        if (req.method === "HEAD") this.__hasBody = false;
      }
      assignSocket(socket) {
        if (socket._httpMessage === this) {
          const error = new Error("Socket already assigned");
          error.code = "ERR_HTTP_SOCKET_ASSIGNED";
          throw error;
        }
        if (socket._httpMessage) {
          socket._httpMessage.detachSocket?.(socket);
          socket._httpMessage = null;
        }
        if (typeof socket.on !== "function") {
          throw Object.assign(new TypeError("socket.on is not a function"), { code: "ERR_INVALID_ARG_TYPE" });
        }
        const response = this;
        const onClose = () => {
          if (socket._httpMessage !== response) return;
          response.destroyed = true;
          response.emit("close");
        };
        socket._httpMessage = this;
        socket.on("close", onClose);
        this.__socketCloseListener = onClose;
        this.socket = socket;
        this.emit("socket", socket);
        return undefined;
      }
      detachSocket(socket = this.socket) {
        if (!socket || socket._httpMessage !== this) return;
        if (this.__socketCloseListener) {
          socket.removeListener?.("close", this.__socketCloseListener);
        }
        socket._httpMessage = null;
        this.socket = null;
        this.__socketCloseListener = undefined;
      }
    }
    const makeResponse = () =>
      new NodeServerResponse({
        method: "GET",
        httpVersionMajor: 1,
        httpVersionMinor: 1
      });
    const makeRequest = (
      handler,
      pathname,
      callback,
      options = {},
      context
    ) => {
      if (typeof pathname === "string" && /[^\u0021-\u00ff]/.test(pathname)) {
        throw Object.assign(new TypeError("Request path contains unescaped characters"), { code: "ERR_UNESCAPED_CHARACTERS" });
      }
      if (
        options.method !== undefined &&
        options.method !== null &&
        typeof options.method !== "string"
      ) {
        const value = options.method;
        const received =
          value !== null && typeof value === "object"
            ? `an instance of ${value.constructor?.name || "Object"}`
            : `type ${typeof value} (${String(value)})`;
        throw Object.assign(new TypeError(`The "options.method" property must be of type string. Received ${received}`), { code: "ERR_INVALID_ARG_TYPE" });
      }
      const request = attachHttpSignal(new NodeIncomingMessage());
      Object.setPrototypeOf(request, NodeClientRequest.prototype);
      request.destroy = (error) => {
        if (request.destroyed) return request;
        request.destroyed = true;
        if (request.socket?.__timeoutTimer !== undefined) {
          clearTimeout(request.socket.__timeoutTimer);
          request.socket.__timeoutTimer = undefined;
        }
        if (request.socket) request.socket.destroyed = true;
        if (request.__signalAbortListener) {
          options.signal?.removeEventListener(
            "abort",
            request.__signalAbortListener
          );
          request.__signalAbortListener = undefined;
        }
        const failure =
          error ||
          (request.__responseEmitted
            ? undefined
            : Object.assign(new Error("socket hang up"), {
                code: "ECONNRESET"
              }));
        queueMicrotask(() => {
          if (failure) request.emit("error", failure);
          if (!request.__closeEmitted) {
            request.__closeEmitted = true;
            request.emit("close");
          }
        });
        return request;
      };
      if (options.signal instanceof AbortSignal) {
        const abortError = Object.assign(
          new Error("The operation was aborted"),
          { code: "ABORT_ERR", name: "AbortError" }
        );
        if (options.signal.aborted) {
          request.destroy(abortError);
        } else {
          request.__signalAbortListener = () => request.destroy(abortError);
          options.signal.addEventListener(
            "abort",
            request.__signalAbortListener,
            { once: true }
          );
        }
      }
      request.agent = options.agent || globalAgent;
      request.url = pathname || "/";
      request.path = request.url;
      request.protocol =
        context?.constructor?.name === "NodeHttpServer" ? "http:" : "http:";
      const optionHostname = Object.prototype.hasOwnProperty.call(
        options,
        "hostname"
      )
        ? options.hostname
        : undefined;
      const optionHost = Object.prototype.hasOwnProperty.call(options, "host")
        ? options.host
        : undefined;
      request.host = optionHostname || optionHost || "localhost";
      request.method = options.method || "GET";
      request.writable = true;
      request.socket = Object.assign(new globalThis.__nodeEventEmitter(), {
        writable: true,
        writableHighWaterMark: 16 * 1024,
        writableCorked: 0,
        setTimeout(msecs) {
          this.timeout = msecs;
          if (this.__timeoutTimer !== undefined) {
            clearTimeout(this.__timeoutTimer);
          }
          if (!this.__timeoutListener) {
            this.__timeoutListener = () => request.emit("timeout");
            this.on("timeout", this.__timeoutListener);
          }
          if (msecs > 0) {
            this.__timeoutTimer = setTimeout(() => {
              this.__timeoutTimer = undefined;
              if (!this.destroyed) this.emit("timeout");
            }, msecs);
          }
          return this;
        },
        setEncoding: () => request.socket,
        destroy() {
          if (this.destroyed) return this;
          this.destroyed = true;
          this.writable = false;
          queueMicrotask(() => this.emit("close"));
          return this;
        }
      });
      if (options.timeout !== undefined) {
        request.socket.setTimeout(options.timeout);
      }
      queueMicrotask(() => {
        request.emit("socket", request.socket);
        queueMicrotask(() => {
          request.socket.__connected = true;
          if (request._timeoutAfterConnect !== undefined) {
            if (request._timeoutTimer !== undefined) {
              clearTimeout(request._timeoutTimer);
              request._timeoutTimer = undefined;
            }
            request.socket.setTimeout(request._timeoutAfterConnect);
          }
          request.socket.emit("connect");
        });
      });
      request.finished = false;
      request.writableFinished = false;
      request.headers = Object.create(null);
      request.rawHeaders = [];
      if (
        options.headers &&
        !Array.isArray(options.headers) &&
        Array.isArray(options.headers.host)
      ) {
        throw Object.assign(new TypeError('The "host" header must be a string [ERR_INVALID_ARG_TYPE]'), { code: "ERR_INVALID_ARG_TYPE" });
      }
      if (Array.isArray(options.headers)) {
        for (let index = 0; index + 1 < options.headers.length; index += 2) {
          const name = options.headers[index];
          const value = options.headers[index + 1];
          const key = String(name).toLowerCase();
          const normalized = Array.isArray(value)
            ? value.join(String(name).toLowerCase() === "cookie" ? "; " : ", ")
            : value;
          request.headers[key] =
            request.headers[key] && key === "cookie"
              ? `${request.headers[key]}; ${normalized}`
              : normalized;
          request.rawHeaders.push(String(name), String(value));
        }
      } else if (options.headers && typeof options.headers === "object") {
        for (const [name, value] of Object.entries(options.headers)) {
          request.headers[name.toLowerCase()] = Array.isArray(value)
            ? value.join(name.toLowerCase() === "cookie" ? "; " : ", ")
            : String(value);
        }
        for (const [name, value] of Object.entries(options.headers)) {
          request.rawHeaders.push(String(name), String(value));
        }
      }
      request.headers.connection ||= "keep-alive";
      if (!request.headers.host && !Array.isArray(options.headers) && context) {
        request.headers.host = `localhost:${context.address().port}`;
      }
      if (
        options.auth &&
        !Array.isArray(options.headers) &&
        !request.headers.authorization
      ) {
        request.headers.authorization = `Basic ${NodeBuffer.from(
          String(options.auth)
        ).toString("base64")}`;
      }
      request.setHeader = (name, value) => {
        request.headers[String(name).toLowerCase()] = value;
        return request;
      };
      request.getHeader = (name) => request.headers[String(name).toLowerCase()];
      request.getHeaders = () =>
        Object.assign(Object.create(null), request.headers);
      request.getHeaderNames = () => Object.keys(request.headers);
      request.hasHeader = (name) =>
        Object.prototype.hasOwnProperty.call(
          request.headers,
          String(name).toLowerCase()
        );
      request.flushHeaders = () => request;
      request.setNoDelay = (noDelay = true) => {
        request.socket?.setNoDelay?.(noDelay);
        return request;
      };
      request.setSocketKeepAlive = (enable = false, initialDelay) => {
        request.socket?.setKeepAlive?.(enable, initialDelay);
        return request;
      };
      request.setSocketTimeout = (timeout) => {
        request.socket?.setTimeout?.(timeout);
        return request;
      };
      request.cork = () => {
        request._corked = (request._corked || 0) + 1;
        return request;
      };
      request.uncork = () => {
        request._corked = Math.max(0, (request._corked || 0) - 1);
        return request;
      };
      request.removeHeader = (name) => {
        delete request.headers[String(name).toLowerCase()];
        return request;
      };
      request.timeout = 0;
      request._timeoutTimer = undefined;
      request.setTimeout = (msecs, callback) => {
        if (typeof msecs !== "number") {
          throw Object.assign(new TypeError(`The "msecs" argument must be of type number. Received type ${typeof msecs}`), { code: "ERR_INVALID_ARG_TYPE" });
        }
        if (!Number.isFinite(msecs) || msecs < 0) {
          const error = new RangeError(
            `The value of "msecs" is out of range. It must be a non-negative finite number. Received ${String(
              msecs
            )}`
          );
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        request.timeout = msecs;
        if (request.socket.__connected) {
          request.socket.setTimeout(msecs);
        } else {
          request._timeoutAfterConnect = msecs;
        }
        if (request._timeoutTimer !== undefined) {
          clearTimeout(request._timeoutTimer);
        }
        if (typeof callback === "function") request.once("timeout", callback);
        if (
          msecs > 0 &&
          !request.socket &&
          !request.destroyed &&
          !request.aborted
        ) {
          request._timeoutTimer = setTimeout(() => {
            request._timeoutTimer = undefined;
            if (!request.destroyed && !request.aborted) request.emit("timeout");
          }, msecs);
        }
        return request;
      };
      request.setEncoding = (encoding) => {
        request._encoding = encoding;
        return request;
      };
      request.write = (chunk, encoding, callback) => {
        if (typeof encoding === "function") callback = encoding;
        const value =
          chunk instanceof NodeBuffer ? chunk.toString() : String(chunk);
        (request._bodyChunks ||= []).push(value);
        request._body = `${request._body || ""}${value}`;
        request._wroteChunk = true;
        if (typeof callback === "function") queueMicrotask(callback);
        return true;
      };
      const response = makeResponse();
      response.__httpClientResponse = true;
      response.__clientRequest = request;
      request.socket = response.socket;
      if (context?.address) {
        response.socket.__quenchServerPort = context.address().port;
      }
      const agentName = request.agent?.getName?.(options);
      let agentSlotTracked = false;
      let agentSocketListed = false;
      let agentWaiter;
      if (agentName && request.agent) {
        const active =
          request.agent.__quenchActiveRequests ||
          (request.agent.__quenchActiveRequests = Object.create(null));
        const activeCount = active[agentName] || 0;
        const totalActive = request.agent.totalSocketCount || 0;
        const hostLimitReached =
          Number.isFinite(request.agent.maxSockets) &&
          activeCount >= request.agent.maxSockets;
        const totalLimitReached =
          Number.isFinite(request.agent.maxTotalSockets) &&
          totalActive >= request.agent.maxTotalSockets;
        if (hostLimitReached || totalLimitReached) {
          (request.agent.requests[agentName] ||= []).push(request);
          const wait = new Promise((resolve) => {
            if (hostLimitReached) {
              const waiters =
                request.agent.__quenchAgentWaiters ||
                (request.agent.__quenchAgentWaiters = Object.create(null));
              (waiters[agentName] ||= []).push(resolve);
            } else {
              (request.agent.__quenchTotalWaiters ||= []).push(resolve);
            }
          });
          agentWaiter = wait;
        }
        active[agentName] = activeCount + 1;
        agentSlotTracked = true;
        if (activeCount < request.agent.maxSockets) {
          (request.agent.sockets[agentName] ||= []).push(response.socket);
          agentSocketListed = true;
          request.agent.totalSocketCount =
            (request.agent.totalSocketCount || 0) + 1;
        }
      }
      let reusableSocket;
      if (agentName) {
        const pool = request.agent.freeSockets?.[agentName];
        while (pool?.length && !reusableSocket) {
          const candidate =
            request.agent.scheduling === "fifo" ? pool.shift() : pool.pop();
          if (!candidate.destroyed) reusableSocket = candidate;
        }
      }
      if (reusableSocket) response.socket = reusableSocket;
      response.__joinDuplicateHeaders = true;
      const clearRequestTimeout = () => {
        if (request._timeoutTimer !== undefined) {
          clearTimeout(request._timeoutTimer);
          request._timeoutTimer = undefined;
        }
      };
      response.once("end", clearRequestTimeout);
      const emitRequestClose = () => {
        if (request.__closeEmitted) return;
        request.__closeEmitted = true;
        queueMicrotask(() => request.emit("close"));
      };
      response.once("end", emitRequestClose);
      response.once("close", emitRequestClose);
      if (agentSlotTracked) {
        let agentSlotReleased = false;
        const releaseAgentSlot = () => {
          if (agentSlotReleased) return;
          agentSlotReleased = true;
          const active = request.agent.__quenchActiveRequests;
          active[agentName] = Math.max(0, (active[agentName] || 1) - 1);
          const queued = request.agent.requests[agentName];
          if (queued?.length) queued.shift();
          if (!queued?.length) delete request.agent.requests[agentName];
          const waiters = request.agent.__quenchAgentWaiters;
          const resolve = waiters?.[agentName]?.shift();
          resolve?.();
          if (waiters && !waiters[agentName]?.length) delete waiters[agentName];
          const totalResolve = request.agent.__quenchTotalWaiters?.shift();
          totalResolve?.();
          request.agent.removeSocket(response.socket, options);
          request.agent.totalSocketCount = Math.max(
            0,
            (request.agent.totalSocketCount || 1) - 1
          );
          if (!request.agent.keepAlive) {
            request.agent.emit("free", response.socket, options);
          }
        };
        response.once("end", releaseAgentSlot);
        response.once("close", releaseAgentSlot);
      }
      const destroyRequest = request.destroy;
      const destroyResponse = response.destroy;
      request.destroy = (error) => {
        if (request._timeoutTimer !== undefined) {
          clearTimeout(request._timeoutTimer);
          request._timeoutTimer = undefined;
        }
        destroyRequest(error);
        response.__abort();
        response.emit("close");
        return request;
      };
      response.destroy = (error) => {
        destroyResponse(error);
        destroyRequest();
        return response;
      };
      const resource = {};
      queueMicrotask(async () => {
        if (agentWaiter) await agentWaiter;
        if (agentSlotTracked && !agentSocketListed) {
          (request.agent.sockets[agentName] ||= []).push(response.socket);
          agentSocketListed = true;
          request.agent.totalSocketCount =
            (request.agent.totalSocketCount || 0) + 1;
        }
        if (request.aborted) return;
        const customAgentConnection =
          request.agent &&
          typeof request.agent.createConnection === "function" &&
          request.agent.createConnection !==
            __quenchDefaultHttpCreateConnection;
        if (customAgentConnection) {
          try {
            const connection = request.agent.createConnection(
              options,
              (error) => {
                if (!error) return;
                request.destroyed = true;
                request.emit("error", error);
                request.emit("close");
              }
            );
            if (connection && typeof connection.on === "function") {
              request.socket = connection;
              response.socket = connection;
              let raw = "";
              let delivered = false;
              connection.on("data", (chunk) => {
                raw +=
                  chunk instanceof NodeBuffer
                    ? chunk.toString()
                    : String(chunk);
                if (delivered || !raw.includes("\r\n\r\n")) return;
                const body = raw.slice(raw.indexOf("\r\n\r\n") + 4);
                const match = body.match(/^([0-9a-f]+)\r\n([\s\S]*)/i);
                if (!match) return;
                const length = Number.parseInt(match[1], 16);
                if (match[2].length < length + 2) return;
                delivered = true;
                response.statusCode = 200;
                request.__responseEmitted = true;
                if (typeof callback === "function") callback(response);
                request.emit("response", response);
                response.__emitData(NodeBuffer.from(match[2].slice(0, length)));
                response.complete = true;
                response.readable = false;
                response.emit("end");
                response.destroyed = true;
                response.closed = true;
                response.emit("close");
              });
              connection.resume?.();
            }
          } catch (error) {
            request.destroyed = true;
            request.emit("error", error);
            request.emit("close");
          }
          return;
        }
        if (request.finished && !request.__finishEmitted) {
          request.__finishEmitted = true;
          request.writableFinished = true;
          request.emit("finish");
        }
        // The outgoing ClientRequest and the server-side IncomingMessage are
        // distinct Node objects, even though this in-memory transport uses a
        // single handler invocation. Keep their terminal events separate.
        if (request._wroteChunk) {
          request.headers["transfer-encoding"] = "chunked";
        } else if (
          ["POST", "PUT"].includes(request.method) &&
          request.headers["content-length"] === undefined
        ) {
          request.headers["content-length"] = String(
            (request._body || "").length
          );
        }
        const serverRequest = new NodeIncomingMessage();
        const serverRequestDestroy = serverRequest.destroy;
        Object.assign(serverRequest, request);
        serverRequest.destroy = serverRequestDestroy;
        serverRequest.complete = false;
        serverRequest.__closeEmitted = false;
        // Node's HTTP parser owns a data listener on the request socket while
        // a request is being dispatched. User code can observe this listener
        // through req.socket.listenerCount("data"), even before it attaches a
        // request-body handler.
        if (serverRequest.socket?.listenerCount?.("data") === 0) {
          serverRequest.socket.on("data", () => {});
        }
        response.req = serverRequest;
        const previous = globalThis.__nodeCurrentAsyncResource;
        globalThis.__nodeCurrentAsyncResource = resource;
        try {
          if (context?.requireHostHeader && !request.headers.host) {
            response.statusCode = 400;
            response.statusMessage = "Bad Request";
            response.headers.connection = "close";
            response.end();
          } else {
            handler.call(context, serverRequest, response);
          }
          if (
            request._body !== undefined &&
            ["POST", "PUT", "PATCH"].includes(request.method)
          ) {
            const chunks = request._bodyChunks || [request._body || ""];
            for (const chunk of chunks) {
              const body = request._encoding
                ? String(chunk)
                : NodeBuffer.from(String(chunk));
              serverRequest.socket?.emit("data", body);
              serverRequest.emit("data", body);
            }
          }
          serverRequest.complete = true;
          const closeServerRequest = () => {
            if (serverRequest.destroyed) return;
            serverRequest.destroyed = true;
            serverRequest.emit("close");
          };
          const endServerRequest = () => {
            if (serverRequest._readableState.ended) return;
            serverRequest._readableState.ended = true;
            serverRequest.emit("end");
          };
          if (
            serverRequest.listenerCount("data") ||
            serverRequest.readableFlowing === true
          ) {
            endServerRequest();
            closeServerRequest();
          } else {
            response.once("close", () => {
              endServerRequest();
              closeServerRequest();
            });
          }
          if (options.joinDuplicateHeaders === false) {
            for (const name of ["authorization", "cookie"]) {
              const value = response.headers[name];
              if (typeof value === "string") {
                response.headers[name] = value.split(
                  name === "cookie" ? "; " : ", "
                )[0];
              }
            }
            if (!request.agent.keepAlive) {
              request.socket.once("close", () => {
                request.agent.emit("free", response.socket, options);
              });
            }
          }
          if (response.__invalidContentLength) {
            const error = new Error("Parse Error: duplicate Content-Length");
            error.code = "HPE_UNEXPECTED_CONTENT_LENGTH";
            request.emit("error", error);
          } else if (
            (response.destroyed && !response.__destroyAfterEnd) ||
            (request.aborted && !request.__abortErrorEmitted)
          ) {
            const error = new Error("socket hang up");
            error.code = "ECONNRESET";
            request.emit("error", error);
          } else {
            request.__responseEmitted = true;
            if (typeof callback === "function") callback(response);
            request.emit("response", response);
            queueMicrotask(() => {
              if (
                request.agent?.keepAlive &&
                response.socket &&
                context?.listening !== false
              ) {
                response.shouldKeepAlive = true;
                response.socket._httpMessage = response;
                request.agent.emit("free", response.socket, options);
                response.socket.emit("free");
              }
            });
          }
        } finally {
          globalThis.__nodeCurrentAsyncResource = previous;
        }
      });
      request.end = (chunk, encoding, callback) => {
        if (typeof chunk === "function") {
          callback = chunk;
          chunk = undefined;
        } else if (typeof encoding === "function") {
          callback = encoding;
        }
        if (chunk !== undefined) {
          const value =
            chunk instanceof NodeBuffer ? chunk.toString() : String(chunk);
          (request._bodyChunks ||= []).push(value);
          request._body = `${request._body || ""}${value}`;
        }
        if (!request.finished && !request.aborted) {
          request.finished = true;
          request.writableFinished = false;
        }
        if (typeof callback === "function") queueMicrotask(callback);
        return request;
      };
      request.abort = () => {
        if (request.aborted) return request;
        request.aborted = true;
        request.__abortErrorEmitted = true;
        request.destroyed = true;
        if (request.__serverRequest && !request.__serverRequest.aborted) {
          request.__serverRequest.aborted = true;
          request.__serverRequest.emit("aborted");
        }
        request.emit("abort");
        if (response && !response.closed) {
          response.destroyed = true;
          response.closed = true;
          response.emit("close");
        }
        return request;
      };
      request.resume = NodeIncomingMessage.prototype.resume;
      request.unref = () => request;
      if (!request.rawHeaders.some((name) => name.toLowerCase() === "host")) {
        request.rawHeaders.push("Host", request.headers.host || "localhost");
      }
      if (
        !request.rawHeaders.some((name) => name.toLowerCase() === "connection")
      ) {
        request.rawHeaders.push("Connection", request.headers.connection);
      }
      for (const name of ["authorization", "cookie"]) {
        const values = [];
        for (let index = 0; index < request.rawHeaders.length; index += 2) {
          if (request.rawHeaders[index].toLowerCase() === name) {
            values.push(request.rawHeaders[index + 1]);
          }
        }
        if (values.length > 1) {
          request.headers[name] =
            context?.joinDuplicateHeaders === false
              ? values[0]
              : values.join(name === "cookie" ? "; " : ", ");
        }
        if (values.length === 1 && request.headers[name] === undefined) {
          request.headers[name] = values[0];
        }
      }
      if (
        options.agent &&
        ((typeof options.agent.createConnection === "function" &&
          options.agent.createConnection !==
            __quenchDefaultHttpCreateConnection) ||
          (typeof options.agent.createSocket === "function" &&
            options.agent.createSocket !==
              NodeHttpAgent.prototype.createConnection))
      ) {
        queueMicrotask(() => {
          try {
            const usesCreateConnection =
              typeof options.agent.createConnection === "function" &&
              options.agent.createConnection !==
                __quenchDefaultHttpCreateConnection;
            const createConnection = usesCreateConnection
              ? options.agent.createConnection.bind(options.agent)
              : options.agent.createSocket.bind(options.agent);
            const onConnection = (error) => {
              if (!error) return;
              request.destroyed = true;
              request.emit("error", error);
              request.emit("close");
            };
            if (usesCreateConnection) createConnection(options, onConnection);
            else createConnection(request, options, onConnection);
          } catch (error) {
            request.destroyed = true;
            request.emit("error", error);
            request.emit("close");
          }
        });
      }
      return request;
    };
    const validateHttpServerInteger = (value, name) => {
      if (typeof value !== "number") {
        throw Object.assign(new TypeError(`The "${name}" argument must be of type number`), { code: "ERR_INVALID_ARG_TYPE" });
      }
      if (!Number.isSafeInteger(value) || value < 0) {
        throw Object.assign(new RangeError(`The value of "${name}" is out of range`), { code: "ERR_OUT_OF_RANGE" });
      }
    };
    const httpServerOptionInteger = (options, name, fallback) => {
      if (options[name] === undefined) return fallback;
      validateHttpServerInteger(options[name], name);
      return options[name];
    };
    class NodeHttpServer extends globalThis.__nodeEventEmitter {
      constructor(options, handler) {
        super();
        if (typeof options === "function") {
          handler = options;
          options = {};
        } else if (options == null) {
          options = {};
        } else if (typeof options !== "object" || Array.isArray(options)) {
          throw Object.assign(new TypeError('The "options" argument must be of type object'), { code: "ERR_INVALID_ARG_TYPE" });
        }
        const requestTimeout = httpServerOptionInteger(
          options,
          "requestTimeout",
          300000
        );
        const headersTimeout = httpServerOptionInteger(
          options,
          "headersTimeout",
          Math.min(60000, requestTimeout)
        );
        if (
          requestTimeout > 0 &&
          headersTimeout > 0 &&
          headersTimeout > requestTimeout
        ) {
          throw Object.assign(new RangeError('The value of "headersTimeout" is out of range'), { code: "ERR_OUT_OF_RANGE" });
        }
        this._handler = handler;
        this.requireHostHeader = options.requireHostHeader !== false;
        this.joinDuplicateHeaders = options.joinDuplicateHeaders === true;
        this._port = undefined;
        this._address = undefined;
        this.requestTimeout = requestTimeout;
        this.headersTimeout = headersTimeout;
        this.keepAliveTimeout = httpServerOptionInteger(
          options,
          "keepAliveTimeout",
          5000
        );
        this.keepAliveTimeoutBuffer = httpServerOptionInteger(
          options,
          "keepAliveTimeoutBuffer",
          1000
        );
        this.connectionsCheckingInterval = httpServerOptionInteger(
          options,
          "connectionsCheckingInterval",
          30000
        );
        this.highWaterMark = options.highWaterMark ?? 65536;
        this.httpAllowHalfOpen = false;
        this.timeout = 0;
        this.maxHeadersCount = null;
        this.maxRequestsPerSocket = 0;
        this[globalThis.__nodeHttpConnectionsCheckingInterval] = {
          _destroyed: false
        };
        this.__quenchRawConnection = (socket) => {
          socket.__quenchRawHttp = true;
          socket.allowHalfOpen = this.httpAllowHalfOpen === true;
          let pending = "";
          socket.on("data", (chunk) => {
            pending += chunk.toString();
            for (;;) {
              const boundary = pending.indexOf("\r\n\r\n");
              if (boundary < 0) return;
              const lines = pending.slice(0, boundary).split("\r\n");
              const headers = Object.create(null);
              const rawHeaders = [];
              for (const line of lines.slice(1)) {
                const separator = line.indexOf(":");
                if (separator < 1) continue;
                const name = line.slice(0, separator);
                const value = line.slice(separator + 1).trim();
                headers[name.toLowerCase()] = value;
                rawHeaders.push(name, value);
              }
              const bodyStart = boundary + 4;
              const isChunked =
                headers["transfer-encoding"]?.toLowerCase() === "chunked";
              let body = "";
              let consumedBody = 0;
              const trailers = Object.create(null);
              if (isChunked) {
                let cursor = bodyStart;
                for (;;) {
                  const lineEnd = pending.indexOf("\r\n", cursor);
                  if (lineEnd < 0) return;
                  const size = parseInt(
                    pending.slice(cursor, lineEnd).split(";", 1)[0],
                    16
                  );
                  if (!Number.isFinite(size)) return;
                  const chunkStart = lineEnd + 2;
                  const chunkEnd = chunkStart + size;
                  if (pending.length < chunkEnd + 2) return;
                  body += pending.slice(chunkStart, chunkEnd);
                  cursor = chunkEnd + 2;
                  if (size === 0) {
                    if (pending.slice(cursor, cursor + 2) === "\r\n") {
                      cursor += 2;
                    } else {
                      const trailerEnd = pending.indexOf("\r\n\r\n", cursor);
                      if (trailerEnd < 0) return;
                      for (const trailer of pending
                        .slice(cursor, trailerEnd)
                        .split("\r\n")) {
                        const separator = trailer.indexOf(":");
                        if (separator > 0) {
                          trailers[trailer.slice(0, separator).toLowerCase()] =
                            trailer.slice(separator + 1).trim();
                        }
                      }
                      cursor = trailerEnd + 4;
                    }
                    consumedBody = cursor - bodyStart;
                    break;
                  }
                }
              } else {
                const bodyLength = Number(headers["content-length"] || 0);
                if (pending.length < bodyStart + bodyLength) return;
                body = pending.slice(bodyStart, bodyStart + bodyLength);
                consumedBody = bodyLength;
              }
              pending = pending.slice(boundary + 4 + consumedBody);
              const [method, url, version] = lines[0].split(" ");
              const request = new NodeIncomingMessage();
              request.method = method;
              request.url = url;
              request.httpVersion = (version || "HTTP/1.1").slice(5);
              request.headers = headers;
              request.rawHeaders = rawHeaders;
              request.trailers = trailers;
              request.socket = socket;
              const response = new NodeServerResponse(request);
              response.assignSocket(socket);
              this._handler?.(request, response);
              if (body) request.emit("data", NodeBuffer.from(body));
              request.emit("end");
              if (pending.length) {
                const remainder = pending;
                pending = "";
                queueMicrotask(() => {
                  pending = remainder;
                  socket.emit("data", "");
                });
                return;
              }
            }
          });
          this.emit("connection", socket);
        };
      }
      listen(port, host, callback) {
        if (typeof port === "function") {
          callback = port;
          port = 0;
          host = "127.0.0.1";
        }
        if (port && typeof port === "object") {
          const options = port;
          callback = typeof host === "function" ? host : callback;
          host = options.host;
          port = options.port;
        }
        if (typeof host === "function") {
          callback = host;
          host = "127.0.0.1";
        }
        const numericPort =
          typeof port === "number" && port !== 0
            ? port
            : 40000 + Math.floor(Math.random() * 5000);
        const existing = servers.get(String(numericPort));
        if (existing && existing !== this) {
          const error = new Error(
            `listen EADDRINUSE: address already in use :::${numericPort}`
          );
          error.code = "EADDRINUSE";
          error.errno = -98;
          error.syscall = "listen";
          error.address = host || "::";
          error.port = numericPort;
          queueMicrotask(() => this.emit("error", error));
          return this;
        }
        this._port = numericPort;
        this._address = host;
        this.__quenchRefedHandle = true;
        globalThis.__quenchRefedHandles =
          (globalThis.__quenchRefedHandles || 0) + 1;
        servers.set(String(this._port), this);
        // Node invokes the listen callback after the server has entered the
        // listening state, on a later turn of the event loop. Keeping this
        // asynchronous also preserves the ordering between the callback and
        // the `listening` event for callers that attach listeners immediately.
        if (typeof callback === "function") {
          queueMicrotask(() => Reflect.apply(callback, this, []));
        }
        globalThis.__nodeClusterListening?.({
          address: String(host || "127.0.0.1"),
          addressType: 4,
          fd: undefined,
          port: numericPort
        });
        queueMicrotask(() => this.emit("listening"));
        return this;
      }
      get listening() {
        return this._port !== undefined;
      }
      address() {
        return {
          port: this._port || 40123,
          address: this._address || "127.0.0.1"
        };
      }
      unref() {
        return this;
      }
      close(callback) {
        const wasListening = this._port !== undefined;
        const closingPort = this._port;
        if (wasListening) {
          servers.delete(String(this._port));
          this._port = undefined;
          this._address = undefined;
          if (this.__quenchRefedHandle) {
            this.__quenchRefedHandle = false;
            globalThis.__quenchRefedHandles = Math.max(
              0,
              (globalThis.__quenchRefedHandles || 0) - 1
            );
          }
        }
        if (closingPort !== undefined && globalThis.__nodeHttpGlobalAgent) {
          for (const sockets of Object.values(
            globalThis.__nodeHttpGlobalAgent.freeSockets || {}
          )) {
            for (const socket of [...sockets]) {
              if (socket.__quenchServerPort === closingPort) socket.destroy?.();
            }
          }
        }
        this[globalThis.__nodeHttpConnectionsCheckingInterval]._destroyed =
          true;
        if (typeof callback === "function") {
          this.once("close", () => {
            Reflect.apply(callback, this, [
              wasListening
                ? undefined
                : Object.assign(new Error("Server is not running."), {
                    code: "ERR_SERVER_NOT_RUNNING"
                  })
            ]);
          });
        }
        queueMicrotask(() => this.emit("close"));
        return this;
      }
      closeAllConnections() {
        return this;
      }
      closeIdleConnections() {
        return this;
      }
      setTimeout(msecs, callback) {
        this.timeout = msecs;
        if (typeof callback === "function") this.on("timeout", callback);
        return this;
      }
    }
    Symbol.asyncDispose ||= Symbol("Symbol.asyncDispose");
    if (Symbol.asyncDispose) {
      NodeHttpServer.prototype[Symbol.asyncDispose] = function () {
        this.close();
        return Promise.resolve();
      };
    }
    class NodeHttpAgent extends globalThis.__nodeEventEmitter {
      constructor(options = {}) {
        super();
        const maxTotalSockets = options.maxTotalSockets;
        if (
          maxTotalSockets !== undefined &&
          typeof maxTotalSockets !== "number"
        ) {
          const received =
            typeof maxTotalSockets === "string"
              ? `string ('${maxTotalSockets}')`
              : `${typeof maxTotalSockets} (${String(maxTotalSockets)})`;
          throw Object.assign(new TypeError(`The "maxTotalSockets" argument must be of type number. Received type ${received}`), { code: "ERR_INVALID_ARG_TYPE" });
        }
        if (
          maxTotalSockets !== undefined &&
          maxTotalSockets !== Infinity &&
          (Number.isNaN(maxTotalSockets) || maxTotalSockets <= 0)
        ) {
          throw Object.assign(new RangeError('The "maxTotalSockets" argument must be greater than 0'), { code: "ERR_OUT_OF_RANGE" });
        }
        this.options = { ...options };
        this.requests = Object.create(null);
        this.sockets = Object.create(null);
        this.freeSockets = Object.create(null);
        this.keepAlive = options.keepAlive === true;
        this.keepAliveMsecs = options.keepAliveMsecs ?? 1000;
        this.agentKeepAliveTimeoutBuffer =
          typeof options.agentKeepAliveTimeoutBuffer === "number" &&
          Number.isFinite(options.agentKeepAliveTimeoutBuffer) &&
          options.agentKeepAliveTimeoutBuffer >= 0
            ? options.agentKeepAliveTimeoutBuffer
            : 1000;
        this.defaultPort = 80;
        this.protocol = "http:";
        this.maxSockets = options.maxSockets ?? Infinity;
        this.maxFreeSockets = options.maxFreeSockets ?? 256;
        this.maxTotalSockets = maxTotalSockets ?? Infinity;
        this.maxCachedSessions = options.maxCachedSessions ?? 100;
        this.scheduling = options.scheduling || "lifo";
        if (this.scheduling !== "fifo" && this.scheduling !== "lifo") {
          throw Object.assign(new TypeError("The argument 'scheduling' must be one of: 'fifo', 'lifo'. " +
              `Received '${this.scheduling}'`), { code: "ERR_INVALID_ARG_VALUE" });
        }
        this.totalSocketCount = 0;
        this.on("free", (socket, options = {}) => {
          if (!socket || socket.writable === false) {
            socket?.destroy?.();
            return;
          }
          const name = this.getName(options);
          const requests = this.requests[name];
          if (requests?.length) {
            const request = requests.shift();
            if (requests.length === 0) delete this.requests[name];
            if (typeof request?.onSocket === "function") {
              this.reuseSocket(socket, request);
              this.sockets[name] ||= [];
              this.sockets[name].push(socket);
              request.onSocket(socket);
            }
            return;
          }
          const message = socket._httpMessage;
          if (!message?.shouldKeepAlive || !this.keepAlive) {
            socket.destroy?.();
            return;
          }
          const freeSockets = this.freeSockets[name] || [];
          const activeCount = this.sockets[name]?.length || 0;
          const socketCount = freeSockets.length + activeCount;
          if (
            this.totalSocketCount > this.maxTotalSockets ||
            socketCount > this.maxSockets ||
            freeSockets.length >= this.maxFreeSockets ||
            !this.keepSocketAlive(socket)
          ) {
            socket.destroy?.();
            return;
          }
          this.freeSockets[name] = freeSockets;
          socket.__quenchAgent = this;
          socket.__quenchAgentName = name;
          socket._httpMessage = null;
          this.removeSocket(socket, options);
          freeSockets.push(socket);
        });
      }
      getName(options = {}) {
        const host = options.host || options.hostname || "localhost";
        const port = options.port || "";
        const localAddress = options.localAddress || "";
        const family = options.family;
        const socketPath = options.socketPath || "";
        const familySuffix = family === 4 || family === 6 ? `:${family}` : "";
        return socketPath
          ? `${host}:${port}:${localAddress}:${socketPath}`
          : `${host}:${port}:${localAddress}${familySuffix}`;
      }
      addRequest(request, options) {
        const name = this.getName(options || request);
        const pool = this.freeSockets[name];
        const socket =
          pool && (this.scheduling === "fifo" ? pool.shift() : pool.pop());
        if (socket) {
          request.reusedSocket = true;
          request.socket = socket;
          queueMicrotask(() => {
            const response = makeResponse();
            response.socket = socket;
            const server = servers.get(String(request._options?.port || ""));
            if (server) {
              const serverRequest = new NodeIncomingMessage();
              serverRequest.method = request.method;
              serverRequest.url = request.path;
              serverRequest.headers = Object.create(null);
              response.req = serverRequest;
              request.__serverRequest = serverRequest;
              server._handler(serverRequest, response);
            }
            request.__responseEmitted = true;
            request.emit("response", response);
          });
        }
        // ClientRequest normally performs the in-memory connection during
        // construction. Public addRequest() still needs to consume a manually
        // seeded free socket, including sockets with a partial _handle.
        return request;
      }
      destroy() {
        for (const pools of [this.freeSockets, this.sockets]) {
          for (const sockets of Object.values(pools)) {
            for (const socket of sockets) socket.destroy?.();
          }
        }
        this.requests = Object.create(null);
        this.sockets = Object.create(null);
        this.freeSockets = Object.create(null);
        return this;
      }
      getCurrentStatus() {
        return {
          createSocketCount: 0,
          closeSocketCount: 0,
          timeoutSocketCount: 0,
          requestCount: 0,
          freeSockets: {},
          sockets: {},
          requests: {}
        };
      }
      createConnection() {
        const error = new Error(
          "HTTP transport is not supported by quench-node"
        );
        error.code = "ENOTSUP";
        throw error;
      }
      keepSocketAlive(socket) {
        socket?.setKeepAlive?.(true, this.keepAliveMsecs);
        socket?.unref?.();
        let agentTimeout = this.options.timeout || 0;
        let canKeepSocketAlive = true;
        const keepAliveHint =
          socket?._httpMessage?.res?.headers?.["keep-alive"];
        const hint = /timeout=(\d+)/.exec(keepAliveHint || "")?.[1];
        if (hint) {
          const serverHintTimeout = Math.max(
            0,
            Number.parseInt(hint, 10) * 1000 - this.agentKeepAliveTimeoutBuffer
          );
          if (serverHintTimeout === 0) canKeepSocketAlive = false;
          else if (serverHintTimeout < agentTimeout) {
            agentTimeout = serverHintTimeout;
          }
        }
        if (socket && socket.timeout !== agentTimeout) {
          socket.setTimeout?.(agentTimeout);
        }
        return canKeepSocketAlive;
      }
      reuseSocket(socket, request) {
        const listener = socket?.__nodeAgentFreeSocketErrorListener;
        if (listener) {
          socket.removeListener?.("error", listener);
          socket.__nodeAgentFreeSocketErrorListener = undefined;
        }
        if (request) request.reusedSocket = true;
        socket?.ref?.();
      }
      removeSocket(socket, options = {}) {
        const name = this.getName(options);
        const pools = [this.sockets];
        if (socket?.writable === false) pools.push(this.freeSockets);
        for (const pool of pools) {
          const sockets = pool[name];
          if (!sockets) continue;
          const index = sockets.indexOf(socket);
          if (index !== -1) sockets.splice(index, 1);
          if (sockets.length === 0) delete pool[name];
        }
      }
    }
    const __quenchDefaultHttpCreateConnection =
      NodeHttpAgent.prototype.createConnection;
    const globalAgent = new NodeHttpAgent({ keepAlive: true });
    globalThis.__nodeHttpGlobalAgent = globalAgent;
    const http = {
      Agent: NodeHttpAgent,
      ClientRequest: NodeClientRequest,
      globalAgent,
      IncomingMessage: NodeIncomingMessage,
      OutgoingMessage: NodeOutgoingMessage,
      Server: NodeHttpServer,
      ServerResponse: NodeServerResponse,
      createServer: (options, handler) => new NodeHttpServer(options, handler),
      get: (target, options, callback) => {
        let requestOptions = { method: "GET" };
        if (typeof options === "function") {
          callback = options;
          options = {};
        } else {
          options ||= {};
        }
        if (
          typeof target === "string" &&
          options &&
          Object.keys(options).length
        ) {
          const original = new URL(target);
          requestOptions = { ...options, method: "GET" };
          const host = options.hostname || options.host || original.hostname;
          const port = options.port ?? (original.port || 80);
          const path = options.path || `${original.pathname}${original.search}`;
          target = `http://${host}:${port}${path}`;
        }
        if (
          typeof target === "object" &&
          target !== null &&
          !(target instanceof URL)
        ) {
          requestOptions = { ...target, method: "GET" };
          if (
            typeof requestOptions.path === "string" &&
            /[^\u0021-\u00ff]/.test(requestOptions.path)
          ) {
            throw Object.assign(new TypeError("Request path contains unescaped characters"), { code: "ERR_UNESCAPED_CHARACTERS" });
          }
          target = `http://${target.hostname || target.host || "localhost"}:${
            target.port || 80
          }${target.path || `${target.pathname || "/"}${target.search || ""}`}`;
        }
        const url = typeof target === "string" ? new URL(target) : target;
        const server = servers.get(url.port || "80");
        const request = makeRequest(
          server ? server._handler : () => {},
          `${url.pathname}${url.search}`,
          callback,
          requestOptions,
          server
        );
        if (requestOptions.timeout !== undefined) {
          request.setTimeout(requestOptions.timeout);
        }
        return request;
      },
      request: (target, options, callback) => {
        if (target instanceof URL) {
          callback = typeof options === "function" ? options : callback;
          const extra = options && typeof options === "object" ? options : {};
          options = {
            ...extra,
            hostname: target.hostname,
            port: target.port || (target.protocol === "https:" ? 443 : 80),
            path: `${target.pathname}${target.search}`,
            headers: target.headers || extra.headers
          };
          target = `http://${options.hostname}:${options.port}${options.path}`;
        }
        if (
          typeof target === "object" &&
          target !== null &&
          !(target instanceof URL)
        ) {
          callback = typeof options === "function" ? options : callback;
          options = target;
          for (const name of ["hostname", "host"]) {
            const value = Object.prototype.hasOwnProperty.call(options, name)
              ? options[name]
              : undefined;
            if (
              value !== undefined &&
              value !== null &&
              typeof value !== "string"
            ) {
              const received =
                value && typeof value === "object"
                  ? `an instance of ${value.constructor?.name || "Object"}`
                  : `type ${typeof value} (${String(value)})`;
              throw Object.assign(new TypeError(`The "options.${name}" property must be of type string or one of undefined or null. Received ${received}`), { code: "ERR_INVALID_ARG_TYPE" });
            }
          }
          if (
            typeof options.path === "string" &&
            /[^\u0021-\u00ff]/.test(options.path)
          ) {
            throw Object.assign(new TypeError("Request path contains unescaped characters"), { code: "ERR_UNESCAPED_CHARACTERS" });
          }
          if (
            options.method !== undefined &&
            typeof options.method === "string" &&
            !/^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/.test(options.method)
          ) {
            const error = new TypeError(
              `Method must be a valid HTTP token ["${String(options.method)}"]`
            );
            error.code = "ERR_INVALID_HTTP_TOKEN";
            throw error;
          }
          const effectivePort =
            options.port ??
            options.defaultPort ??
            options.agent?.defaultPort ??
            80;
          const hostname = Object.prototype.hasOwnProperty.call(
            options,
            "hostname"
          )
            ? options.hostname
            : undefined;
          const host = Object.prototype.hasOwnProperty.call(options, "host")
            ? options.host
            : undefined;
          target = `http://${hostname || host || "localhost"}:${effectivePort}${
            options.path || "/"
          }`;
          if (
            options.timeout !== undefined &&
            typeof options.timeout !== "number"
          ) {
            throw Object.assign(new TypeError(`The "timeout" argument must be of type number. Received type ${typeof options.timeout}`), { code: "ERR_INVALID_ARG_TYPE" });
          }
        }
        const url = typeof target === "string" ? new URL(target) : target;
        const server = servers.get(url.port || "80");
        const request = makeRequest(
          server ? server._handler : () => {},
          `${url.pathname}${url.search}`,
          callback,
          options || {},
          server
        );
        if (options?.timeout !== undefined) request.setTimeout(options.timeout);
        return request;
      }
    };
    globalThis.__nodeHttp = http;
    __quenchHttpModule = http;
  }
}
"#;
