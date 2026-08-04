globalThis.__nodeCommon = {
  mustCall: (fn = () => {}, exact = 1) => {
    let calls = 0;
    const wrapped = function (...args) {
      calls++;
      wrapped.calls = calls;
      return fn.apply(this, args);
    };
    wrapped.calls = 0;
    wrapped.expected = exact;
    wrapped.__quench_index = (globalThis.__nodeCallChecks ||= []).length;
    globalThis.__nodeCallChecks.push(wrapped);
    return wrapped;
  },
  mustCallAtLeast: (fn, minimum = 1) => {
    const wrapped = globalThis.__nodeCommon.mustCall(fn, minimum);
    wrapped.__quench_at_least = true;
    return wrapped;
  },
  mustSucceed: (fn = () => {}) =>
    globalThis.__nodeCommon.mustCall((error, ...args) => {
      if (error) throw error;
      return fn(...args);
    }),
  mustNotCall:
    (message = "Unexpected call") =>
    () => {
      throw new Error(message);
    },
  noop: () => {},
  isAlive: (pid) => {
    const alive = globalThis.__quench_node_pids || new Set();
    globalThis.__quench_node_pids = alive;
    return alive.has(pid);
  },
  printSkipMessage: (message) => console.log(`# SKIP: ${message}`),
  expectsError: (_expected) => (error) => {
    if (!error) throw new Error("Expected filesystem error");
  },
  invalidArgTypeHelper: (input) => {
    if (input == null) return ` Received ${input}`;
    let rendered;
    try {
      rendered = String(input);
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
  getArrayBufferViews: (buffer) => [
    buffer,
    new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength),
    new DataView(buffer.buffer, buffer.byteOffset, buffer.byteLength)
  ]
};
globalThis.__quench_verify_calls = () => {
  for (const callback of globalThis.__nodeCallChecks || []) {
    if (
      callback.__quench_at_least
        ? callback.calls < callback.expected
        : callback.calls !== callback.expected
    )
      throw new Error(
        `Callback ${callback.__quench_index}: expected ${callback.expected} calls, got ${callback.calls}`
      );
  }
};
globalThis.__nodeTmpdir = {
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
      `file://${globalThis.__nodePath.join(globalThis.__nodeTmpdir.path, String(name))}`
    )
};
class NodeEventEmitter {
  constructor(options = {}) {
    this._events = Object.create(null);
    this.captureRejections =
      options.captureRejections ?? NodeEventEmitter.captureRejections ?? false;
  }
  on(event, listener) {
    this._events ||= Object.create(null);
    const current = this._events[event];
    this._events[event] =
      current === undefined
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
    if (event === "error") {
      const monitorSymbol =
        globalThis.__nodeErrorMonitorSymbol ||
        Symbol.for("events.errorMonitor");
      this.listeners(monitorSymbol).forEach((listener) => listener(...args));
    }
    const listeners = this._events[event];
    const values =
      listeners === undefined
        ? []
        : Array.isArray(listeners)
          ? listeners
          : [listeners];
    values.slice().forEach((listener) => {
      const result = listener.call(this, ...args);
      if (this.captureRejections && result?.then)
        result.catch((error) =>
          queueMicrotask(() => {
            const rejection = this[Symbol.for("nodejs.rejection")];
            if (typeof rejection === "function")
              rejection.call(this, error, event, ...args);
            else this.emit("error", error);
          })
        );
    });
    return values.length > 0;
  }
  removeListener(event, listener) {
    const current = this.listeners(event);
    const removed = current.find(
      (item) => item === listener || item.listener === listener
    );
    if (!removed) return this;
    const values = current.filter((item) => item !== removed);
    if (values.length === 0) delete this._events[event];
    else this._events[event] = values.length === 1 ? values[0] : values;
    if (event !== "removeListener")
      this.emit("removeListener", event, removed.listener || removed);
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
    for (const name of names)
      for (const listener of this.listeners(name).reverse())
        this.removeListener(name, listener);
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
globalThis.__nodeEventEmitter = NodeEventEmitter;
globalThis.process._events = Object.create(null);
