globalThis.EventTarget ||= class EventTarget {
  constructor() {
    this._listeners = {};
  }
  addEventListener(name, listener) {
    (this._listeners[name] ||= []).push(listener);
  }
  removeEventListener(name, listener) {
    this._listeners[name] = (this._listeners[name] || []).filter(
      (item) => item !== listener
    );
  }
  dispatchEvent(event) {
    for (const listener of this._listeners[event.type] || [])
      listener.call(this, event);
    return true;
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
