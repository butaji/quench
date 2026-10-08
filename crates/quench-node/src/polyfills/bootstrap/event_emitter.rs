//! Shared guest bridge for Node's EventEmitter listener semantics.

pub const JS: &str = quench_js_check::checked_js!(r#"const MAX_LISTENERS = Symbol("maxListeners");
const ERROR_INSPECT = Symbol.for("quench.internal.eventEmitter.inspect");
const STREAM_OWNER = Symbol.for("quench.internal.streamOwner");
const checkListener = (listener) => {
  if (typeof listener === "function") return;
  const received = listener === null
    ? "null"
    : listener === undefined
    ? "undefined"
    : typeof listener === "string"
    ? `type string ('${listener.replaceAll("'", "\\'")}')`
    : typeof listener === "object"
    ? `an instance of ${Array.isArray(listener) ? "Array" : "Object"}`
    : `type ${typeof listener} (${String(listener)})`;
  const error = new TypeError(
    `The "listener" argument must be of type function. Received ${received}`,
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
const registerListener = (emitter, event, listener, prepend) => {
  checkListener(listener);
  emitter._events ||= Object.create(null);
  if (event !== "newListener" && emitter._events.newListener !== undefined) {
    emitter.emit("newListener", event, listener.listener ?? listener);
  }
  const current = emitter._events[event];
  const listeners = current === undefined
    ? []
    : Array.isArray(current)
    ? current
    : [current];
  const next = prepend
    ? [listener, ...listeners]
    : [...listeners, listener];
  emitter._events[event] = next.length === 1 ? listener : next;
  return emitter;
};
const registerOnce = (emitter, event, listener, prepend) => {
  checkListener(listener);
  let called = false;
  const wrapped = (...args) => {
    if (called) return;
    called = true;
    emitter.removeListener(event, wrapped);
    return Reflect.apply(listener, emitter, args);
  };
  wrapped.listener = listener;
  return registerListener(emitter, event, wrapped, prepend);
};
class EventEmitter {
  constructor(options = {}) {
    this._events = Object.create(null);
    const activeDomain = globalThis.__quench_active_domain;
    if (activeDomain) {
      this.domain = activeDomain;
      activeDomain.add(this);
    }
    this.captureRejections = options.captureRejections ??
      EventEmitter.captureRejections ?? false;
  }
  addListener(event, listener) {
    return registerListener(this, event, listener, false);
  }
  once(event, listener) {
    return registerOnce(this, event, listener, false);
  }
  prependListener(event, listener) {
    return registerListener(this, event, listener, true);
  }
  prependOnceListener(event, listener) {
    return registerOnce(this, event, listener, true);
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
    const internalOnlyError = event === "error" && values.length > 0 &&
      values.every((listener) => listener.__quenchInternal === true);
    const ownerWasDestroyed = this[STREAM_OWNER]?.destroyed === true;
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
    if (
      event === "error" &&
      (values.length === 0 || (internalOnlyError && ownerWasDestroyed))
    ) {
      if (internalOnlyError) {
        values.slice().filter((listener) => typeof listener === "function").forEach(
          (listener) => Reflect.apply(listener, this, args),
        );
      }
      const error = args[0];
      if (error instanceof Error) throw error;
      let detail;
      try {
        const inspect = EventEmitter[ERROR_INSPECT];
        detail = typeof inspect === "function" ? inspect(error) : String(error);
      } catch {
        detail = String(error);
      }
      const unhandled = new Error(`Unhandled error. (${detail})`);
      unhandled.code = "ERR_UNHANDLED_ERROR";
      unhandled.context = error;
      throw unhandled;
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
    checkListener(listener);
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
    return this[MAX_LISTENERS] ?? EventEmitter.defaultMaxListeners;
  }
  setMaxListeners(limit) {
    if (typeof limit !== "number") {
      const error = new TypeError("The \"n\" argument must be of type number");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isFinite(limit) || limit < 0) {
      const error = new RangeError("The value of \"n\" is out of range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    this[MAX_LISTENERS] = limit;
    return this;
  }
}
const aliasMethod = (source, alias) => {
  const descriptor = Object.getOwnPropertyDescriptor(
    EventEmitter.prototype,
    source,
  );
  Object.defineProperty(EventEmitter.prototype, alias, descriptor);
};
aliasMethod("addListener", "on");
aliasMethod("removeListener", "off");
for (const name of Object.getOwnPropertyNames(EventEmitter.prototype)) {
  if (name === "constructor") continue;
  const descriptor = Object.getOwnPropertyDescriptor(EventEmitter.prototype, name);
  if (typeof descriptor.value === "function") {
    Object.defineProperty(EventEmitter.prototype, name, {
      ...descriptor,
      enumerable: true,
    });
  }
}
EventEmitter.defaultMaxListeners = 10;
EventEmitter.captureRejectionSymbol = Symbol.for("nodejs.rejection");
Object.defineProperty(globalThis, "__nodeEventEmitter", {
  value: EventEmitter,
  configurable: true,
});"#);
