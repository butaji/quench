//! Polyfill: `support`

pub const JS: &str = quench_js_check::checked_js!(r#"globalThis.gc ||= (typeof gc === "function" ? gc : function () { return undefined; });
Object.defineProperty(globalThis, "__nodeCallChecks", {
  value: [],
  writable: true,
  configurable: true,
});
Object.defineProperty(globalThis, "__nodeCommon", { value: {
  mustCall: (fn = () => {}, exact = 1) => {
    if (typeof fn === "number") {
      exact = fn;
      fn = () => {};
    }
    let calls = 0;
    const wrapped = function () {
      "use strict";
      const args = Array.from(arguments);
      calls++;
      wrapped.calls = calls;
      try {
        return fn.apply(this, args);
      } catch (error) {
        throw error;
      }
    };
    wrapped.calls = 0;
    wrapped.expected = exact;
    wrapped.__quench_index = (globalThis.__nodeCallChecks ||= []).length;
    const checks = globalThis.__nodeCallChecks ||= [];
    checks[checks.length] = wrapped;
    return wrapped;
  },
  mustCallAtLeast: (fn, minimum = 1) => {
    const wrapped = globalThis.__nodeCommon.mustCall(fn, minimum);
    wrapped.__quench_at_least = true;
    return wrapped;
  },
  mustSucceed: (fn = () => {}, exact = 1) =>
    globalThis.__nodeCommon.mustCall((error, ...args) => {
      if (error) throw error;
      return fn(...args);
    }, exact),
  mustNotCall: (message = "Unexpected call") => () => {
    throw new Error(message);
  },
  noop: () => {},
  spawnPromisified: (...args) => {
    const child = globalThis.require("child_process").spawn(...args);
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (data) => (stdout += data));
    child.stderr.on("data", (data) => (stderr += data));
    return new Promise((resolve, reject) => {
      child.on("close", (code, signal) =>
        resolve({
          code,
          signal,
          stdout,
          stderr,
        }));
      child.on("error", (error) =>
        reject({
          code: error,
          signal: undefined,
          stdout,
          stderr,
        }));
    });
  },
  platformTimeout: (milliseconds) => milliseconds,
  pwdCommand: ["pwd", []],
  escapePOSIXShell: (parts, ...values) => {
    const env = { ...process.env };
    let command = parts[0];
    for (let index = 0; index < values.length; index++) {
      const name = `ESCAPED_${index}`;
      env[name] = values[index];
      command += `\${${name}}${parts[index + 1]}`;
    }
    return [command, { env }];
  },
  allowGlobals: (..._values) => {},
  isAlive: (pid) => {
    const alive = globalThis.__quench_node_pids || new Set();
    globalThis.__quench_node_pids = alive;
    return alive.has(pid);
  },
  printSkipMessage: (message) => console.log(`# SKIP: ${message}`),
  skipIfInspectorDisabled: () =>
    globalThis.__nodeCommon.skip("inspector disabled"),
  skipIf32Bits: () => {
    if (process.arch === "ia32" || process.arch === "arm") {
      globalThis.__nodeCommon.skip("32-bit platform");
    }
  },
  skip: (message = "") => {
    console.log(`1..0 # Skipped: ${message}`);
    process.exit(0);
  },
  hasCrypto: true,
  localhostIPv4: "127.0.0.1",
  localhostIPv6: "::1",
  hasIPv6: true,
  // `common.getTTYfd` is a test-helper fact, not a second Node runtime.  Keep
  // the probe on the Rust-owned tty/fs capabilities so every common import
  // observes the same host result.
  getTTYfd: () => {
    const tty = globalThis.require("tty");
    for (const fd of [0, 1, 2]) {
      if (tty.isatty(fd)) return fd;
    }
    try {
      return globalThis.require("fs").openSync("/dev/tty");
    } catch (_) {
      return -1;
    }
  },
  // Keep the socket fixture name in the shared helper object so every
  // `require('../common')` spelling observes the same path fact.
  PIPE: `node-test.${process.pid}.sock`,
  expectsError: (expected, exact = 1) => globalThis.__nodeCommon.mustCall((error) => {
    if (!error) throw new Error("Expected filesystem error");
    if (typeof expected === "function") {
      const result = expected(error);
      if (result === false) throw new Error("Error validation failed");
      return true;
    }
    if (expected && typeof expected === "object") {
      for (const key of Object.keys(expected)) {
        const wanted = expected[key];
        const actual = error[key];
        if (wanted instanceof RegExp ? !wanted.test(actual) : actual !== wanted) {
          throw new Error(`Unexpected error ${key}`);
        }
      }
    }
    return true;
  }, exact),
  invalidArgTypeHelper: (input) => {
    if (input == null) return ` Received ${input}`;
    if (typeof input === "string") return ` Received type string ('${input}')`;
    if (typeof input === "function") {
      return ` Received function ${input.name}`;
    }
    if (typeof input === "object") {
      if (Object.getPrototypeOf(input) === null) {
        return " Received [Object: null prototype] {}";
      }
      return ` Received an instance of ${input.constructor?.name || "Object"}`;
    }
    let rendered;
    try {
      rendered = typeof input === "bigint"
        ? `${String(input)}n`
        : String(input);
    } catch (_) {
      rendered = Object.prototype.toString.call(input);
    }
    return ` Received type ${typeof input} (${rendered})`;
  },
  expectWarning: (_type, _message) => {},
  mustNotMutateObjectDeep: (value) => value,
  isLinux: process.platform === "linux",
  hasIntl: typeof Intl !== "undefined",
  isDebug: false,
  isMacOS: process.platform === "darwin",
  isWindows: process.platform === "win32",
  isAIX: false,
  isFreeBSD: false,
  enoughTestMem: true,
  canCreateSymLink: () => process.platform !== "win32",
  // Keep the view order shared with getBufferSources.  The test helper uses
  // the same index to verify every binary view handed to a stream callback.
  getArrayBufferViews: (buffer) => {
    const bytes = buffer instanceof ArrayBuffer
      ? new Uint8Array(buffer)
      : new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
    const start = bytes.byteOffset;
    const length = bytes.byteLength;
    const source = bytes.buffer;
    return [
      new Int8Array(source, start, length),
      new Uint8Array(source, start, length),
      new Uint8ClampedArray(source, start, length),
      new DataView(source, start, length),
    ];
  },
  getBufferSources: (buffer) => {
    const bytes = buffer instanceof ArrayBuffer
      ? new Uint8Array(buffer)
      : new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
    const start = bytes.byteOffset;
    const length = bytes.byteLength;
    const source = bytes.buffer;
    return [
      new Int8Array(source, start, length),
      new Uint8Array(source, start, length),
      new Uint8ClampedArray(source, start, length),
      new DataView(source, start, length),
      source.slice(start, start + length),
    ];
  },
}, configurable: true });
Object.defineProperty(globalThis, "__quench_verify_calls", { value: () => {
  for (const callback of globalThis.__nodeCallChecks || []) {
    if (
      callback.__quench_at_least
        ? callback.calls < callback.expected
        : callback.calls !== callback.expected
    ) {
      throw new Error(
        `Callback ${callback.__quench_index}: expected ${callback.expected} calls, got ${callback.calls}`,
      );
    }
  }
}, configurable: true });
Object.defineProperty(globalThis, "__nodeTmpdir", { value: {
  path: `/tmp/quench-node-${process.pid}`,
  hasEnoughSpace: (_bytes) => false,
  refresh: () => {
    try {
      globalThis.__quench_fs_mkdir(globalThis.__nodeTmpdir.path);
    } catch (_) {}
  },
  resolve: (name = "") =>
    globalThis.__nodePath.join(globalThis.__nodeTmpdir.path, String(name)),
  fileURL: (name = "") =>
    new globalThis.__nodeURL(
      `file://${
        globalThis.__nodePath.join(
          globalThis.__nodeTmpdir.path,
          String(name),
        )
      }`,
    ),
}, configurable: true });
class NodeEventEmitter {
  constructor(options = {}) {
    this._events = Object.create(null);
    const activeDomain = globalThis.__quench_active_domain;
    if (activeDomain) {
      this.domain = activeDomain;
      activeDomain.add(this);
    }
    this.captureRejections = options.captureRejections ??
      NodeEventEmitter.captureRejections ?? false;
  }
  on(event, listener) {
    this._events ||= Object.create(null);
    const current = this._events[event];
    this._events[event] = current === undefined
      ? listener
      : Array.isArray(current)
      ? [...current, listener]
      : [current, listener];
    return this;
  }
  addListener(event, listener) {
    return this.on(event, listener);
  }
  once(event, listener) {
    const wrapped = (...args) => {
      this.removeListener(event, wrapped);
      listener(...args);
    };
    return this.on(event, wrapped);
  }
  emit(event, ...args) {
    this._events ||= Object.create(null);
    if (event === "error") {
      const monitorSymbol = globalThis.__nodeErrorMonitorSymbol ||
        Symbol.for("events.errorMonitor");
      this.listeners(monitorSymbol).forEach((listener) =>
        Reflect.apply(listener, this, args)
      );
    }
    const listeners = this._events[event];
    const values = listeners === undefined
      ? []
      : Array.isArray(listeners)
      ? listeners
      : [listeners];
    if (event === "error" && values.length === 0 && this.domain) {
      const error = args[0] && typeof args[0] === "object"
        ? args[0]
        : Object.assign(new Error("Unhandled error."), { domain: this.domain });
      error.domain = this.domain;
      error.domainEmitter = this;
      error.domainThrown = false;
      this.domain.emit("error", error);
      return true;
    }
    values.slice().filter((listener) => typeof listener === "function").forEach(
      (listener) => {
        const result = Reflect.apply(listener, this, args);
        if (result?.then) {
          result.catch((error) => setImmediate(() => {
            if (this.captureRejections && event !== "error") {
              const rejection = this[Symbol.for("nodejs.rejection")];
              if (typeof rejection === "function") {
                rejection.call(this, error, event, ...args);
              } else this.emit("error", error);
            } else {
              process.emit("unhandledRejection", error);
            }
          }));
        }
      },
    );
    return values.length > 0;
  }
  removeListener(event, listener) {
    const current = this.listeners(event);
    const removed = current.find(
      (item) => item === listener || item.listener === listener,
    );
    if (!removed) return this;
    const values = current.filter((item) => item !== removed);
    if (values.length === 0) delete this._events[event];
    else this._events[event] = values.length === 1 ? values[0] : values;
    if (event !== "removeListener") {
      this.emit("removeListener", event, removed.listener || removed);
    }
    return this;
  }
  off(event, listener) {
    return this.removeListener(event, listener);
  }
  removeAllListeners(event) {
    if (!this._events) {
      this._events = Object.create(null);
      return this;
    }
    const names = event === undefined ? this.eventNames() : [event];
    if (event === undefined && names.includes("removeListener")) {
      names.splice(names.indexOf("removeListener"), 1);
      names.push("removeListener");
    }
    for (const name of names) {
      for (const listener of this.listeners(name).reverse()) {
        this.removeListener(name, listener);
      }
    }
    return this;
  }
  listeners(event) {
    if (event === undefined || !this._events) return [];
    const value = this._events[event];
    return value === undefined
      ? []
      : Array.isArray(value)
      ? value.slice()
      : [value];
  }
  listenerCount(event) {
    return this.listeners(event).length;
  }
}
NodeEventEmitter.captureRejectionSymbol = Symbol.for("nodejs.rejection");
Object.defineProperty(globalThis, "__nodeEventEmitter", {
  value: NodeEventEmitter,
  configurable: true,
});
globalThis.process._events = Object.create(null);
"#);
