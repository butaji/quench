//! Shared guest bridge for Node's EventEmitter listener semantics.

pub const JS: &str = quench_js_check::checked_js!(
    r#"class NodeEventEmitter {
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
    let called = false;
    const wrapped = (...args) => {
      if (called) return;
      called = true;
      this.removeListener(event, wrapped);
      return Reflect.apply(listener, this, args);
    };
    wrapped.listener = listener;
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
  getMaxListeners() {
    return this._maxListeners ?? NodeEventEmitter.defaultMaxListeners;
  }
  setMaxListeners(limit) {
    if (!Number.isInteger(limit) || limit < 0) {
      throw Object.assign(new RangeError('The value of "n" is out of range'), {
        code: 'ERR_OUT_OF_RANGE',
      });
    }
    this._maxListeners = limit;
    return this;
  }
}
NodeEventEmitter.defaultMaxListeners = 10;
NodeEventEmitter.getMaxListeners = (emitter) => emitter.getMaxListeners();
NodeEventEmitter.setMaxListeners = (limit, ...emitters) => {
  if (emitters.length === 0) {
    if (!Number.isInteger(limit) || limit < 0) {
      throw Object.assign(new RangeError('The value of "n" is out of range'), {
        code: 'ERR_OUT_OF_RANGE',
      });
    }
    NodeEventEmitter.defaultMaxListeners = limit;
  } else {
    for (const emitter of emitters) emitter.setMaxListeners(limit);
  }
  return NodeEventEmitter;
};
NodeEventEmitter.captureRejectionSymbol = Symbol.for("nodejs.rejection");
Object.defineProperty(globalThis, "__nodeEventEmitter", {
  value: NodeEventEmitter,
  configurable: true,
});"#
);
