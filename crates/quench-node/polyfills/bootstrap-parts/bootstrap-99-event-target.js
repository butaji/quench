globalThis.EventTarget ||= class EventTarget {
  constructor() {
    this._listeners = {};
  }
  addEventListener(name, listener, options = {}) {
    if (typeof listener !== "function") return undefined;
    const signal = options.signal;
    if (signal !== undefined && !(signal instanceof AbortSignal))
      throw new TypeError("signal must be an AbortSignal");
    if (signal?.aborted) return undefined;
    const record = { listener, once: Boolean(options.once), signal };
    (this._listeners[name] ||= []).push(record);
    if (signal) {
      record.abort = () => this.removeEventListener(name, listener);
      signal.addEventListener("abort", record.abort, { once: true });
    }
    return undefined;
  }
  removeEventListener(name, listener) {
    this._listeners[name] = (this._listeners[name] || []).filter((record) => {
      if (record.listener !== listener) return true;
      if (record.abort)
        record.signal?.removeEventListener("abort", record.abort);
      return false;
    });
  }
  dispatchEvent(event) {
    for (const record of [...(this._listeners[event.type] || [])]) {
      if (!this._listeners[event.type]?.includes(record)) continue;
      if (record.once) this.removeEventListener(event.type, record.listener);
      record.listener.call(this, event);
      if (event._quenchImmediatePropagationStopped) break;
    }
    return true;
  }
};
globalThis.Event ||= class Event {
  constructor(type, options = {}) {
    this.type = String(type);
    this.bubbles = Boolean(options.bubbles);
    this.cancelable = Boolean(options.cancelable);
    this.composed = Boolean(options.composed);
    this.defaultPrevented = false;
    this._quenchImmediatePropagationStopped = false;
  }
  preventDefault() {
    if (this.cancelable) this.defaultPrevented = true;
  }
  stopImmediatePropagation() {
    this._quenchImmediatePropagationStopped = true;
  }
};
if (globalThis.Event && !Event.prototype.stopImmediatePropagation)
  Event.prototype.stopImmediatePropagation = function () {
    this._quenchImmediatePropagationStopped = true;
  };
if (globalThis.AbortSignal && !AbortSignal.prototype.__quenchEventArgument) {
  const originalAddEventListener = AbortSignal.prototype.addEventListener;
  AbortSignal.prototype.addEventListener = function (type, listener, options) {
    if (typeof listener !== "function")
      return originalAddEventListener.call(this, type, listener, options);
    return originalAddEventListener.call(
      this,
      type,
      (event) =>
        listener.call(this, event || { stopImmediatePropagation() {} }),
      options
    );
  };
  AbortSignal.prototype.__quenchEventArgument = true;
}
const __quenchEventsOriginalRequire = globalThis.require;
let __quenchEventsModule;
const __quenchEventsTargetValid = (target) =>
  target instanceof __quenchEventsModule ||
  target instanceof EventTarget ||
  target instanceof AbortSignal;
const __quenchValidateEventLimit = (limit) => {
  if (typeof limit !== "number") {
    const error = new TypeError(
      "The setMaxListeners argument must be a number [ERR_INVALID_ARG_TYPE]"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (Number.isNaN(limit) || limit < 0) {
    const error = new RangeError(
      "The value of setMaxListeners is out of range [ERR_OUT_OF_RANGE]"
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __quenchEventsRequire = (value) => {
  if (__quenchEventsModule) return __quenchEventsModule;
  const limits = new WeakMap();
  __quenchEventsModule = value.EventEmitter;
  Object.assign(__quenchEventsModule, value);
  __quenchEventsModule.defaultMaxListeners = 10;
  __quenchEventsModule.getMaxListeners = (target) => {
    if (!__quenchEventsTargetValid(target)) {
      const error = new TypeError(
        "The eventTarget argument must be an instance of EventEmitter or EventTarget [ERR_INVALID_ARG_TYPE]"
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (target instanceof AbortSignal) return 0;
    return limits.get(target) ?? 10;
  };
  __quenchEventsModule.setMaxListeners = (limit, ...targets) => {
    __quenchValidateEventLimit(limit);
    for (const target of targets) {
      if (!__quenchEventsTargetValid(target)) {
        const error = new TypeError(
          "The eventTargets argument must be an instance of EventEmitter or EventTarget [ERR_INVALID_ARG_TYPE]"
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      limits.set(target, limit);
    }
    return targets[0];
  };
  return __quenchEventsModule;
};
globalThis.require = (name) => {
  const value = __quenchEventsOriginalRequire(name);
  if (String(name).replace(/^node:/, "") !== "events") return value;
  return __quenchEventsRequire(value);
};
