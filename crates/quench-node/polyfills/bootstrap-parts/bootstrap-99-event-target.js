globalThis.EventTarget ||= class EventTarget {
  constructor() {
    this._listeners = {};
  }
  addEventListener(name, listener, options = {}) {
    const passive = Boolean(options.passive);
    if (typeof listener !== "function") return undefined;
    const signal = options.signal;
    if (signal !== undefined && !(signal instanceof AbortSignal))
      throw new TypeError("signal must be an AbortSignal");
    if (signal?.aborted) return undefined;
    const record = { listener, once: Boolean(options.once), passive, signal };
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
      event._quenchPassive = record.passive;
      record.listener.call(this, event);
      event._quenchPassive = false;
      if (event._quenchImmediatePropagationStopped) break;
    }
    return true;
  }
};
globalThis.Event ||= class Event {
  constructor(type, options = {}) {
    if (type === undefined) throw new TypeError("Event type is required");
    this.type = String(type);
    this.bubbles = Boolean(options.bubbles);
    this.cancelable = Boolean(options.cancelable);
    this.composed = Boolean(options.composed);
    this.defaultPrevented = false;
    this._quenchImmediatePropagationStopped = false;
  }
  preventDefault() {
    if (this.cancelable && !this._quenchPassive) this.defaultPrevented = true;
  }
  stopImmediatePropagation() {
    this._quenchImmediatePropagationStopped = true;
  }
};
globalThis.CustomEvent ||= class CustomEvent extends Event {
  constructor(type, options = {}) {
    if (type === undefined || typeof type === "symbol")
      throw new TypeError("CustomEvent type is invalid");
    if (options === null || typeof options !== "object") {
      const error = new TypeError(
        `The "options" argument must be of type object. Received type ${typeof options} (${String(options)})`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    super(type, options);
    Object.defineProperty(this, "detail", {
      value: options.detail === undefined ? null : options.detail,
      enumerable: true
    });
  }
  get [Symbol.toStringTag]() {
    return "CustomEvent";
  }
};
const __quenchEventTargetAdd = EventTarget.prototype.addEventListener;
const __quenchEventTargetRemove = EventTarget.prototype.removeEventListener;
const __quenchEventTargetDispatch = EventTarget.prototype.dispatchEvent;
const __quenchPassiveListeners = new WeakMap();
EventTarget.prototype.addEventListener = function (name, listener, options) {
  const passive = Boolean(options?.passive);
  if (!passive || typeof listener !== "function")
    return __quenchEventTargetAdd.call(this, name, listener, options);
  const wrapper = (event) => {
    event._quenchPassive = true;
    listener.call(this, event);
    event._quenchPassive = false;
  };
  let listeners = __quenchPassiveListeners.get(this);
  if (!listeners) __quenchPassiveListeners.set(this, (listeners = new Map()));
  listeners.set(listener, wrapper);
  return __quenchEventTargetAdd.call(this, name, wrapper, options);
};
EventTarget.prototype.removeEventListener = function (name, listener, options) {
  const wrapper = __quenchPassiveListeners.get(this)?.get(listener) || listener;
  __quenchPassiveListeners.get(this)?.delete(listener);
  return __quenchEventTargetRemove.call(this, name, wrapper, options);
};
EventTarget.prototype.dispatchEvent = function (event) {
  event.target = this;
  event.currentTarget = this;
  try {
    Object.defineProperty(event, "eventPhase", {
      value: 2,
      configurable: true
    });
  } catch (_) {}
  event._quenchPath = [this];
  const result = __quenchEventTargetDispatch.call(this, event);
  return result && !event.defaultPrevented;
};
if (globalThis.Event && !Event.prototype.stopImmediatePropagation)
  Event.prototype.stopImmediatePropagation = function () {
    this._quenchImmediatePropagationStopped = true;
  };
if (globalThis.Event && !Event.prototype.__quenchPassivePreventDefault) {
  const originalPreventDefault = Event.prototype.preventDefault;
  Event.prototype.preventDefault = function () {
    if (!this._quenchPassive) originalPreventDefault.call(this);
  };
  Event.prototype.__quenchPassivePreventDefault = true;
}
if (globalThis.Event) {
  Event.prototype.timeStamp ||= Date.now();
  Event.prototype.composedPath ||= function () {
    return this._quenchPath || [];
  };
  Event.prototype.returnValue ??= true;
  Event.prototype.isTrusted ??= false;
  Event.prototype.eventPhase ??= 0;
  Event.prototype.cancelBubble ??= false;
  try {
    Object.defineProperty(Event.prototype, "cancelBubble", {
      get() {
        return Boolean(this._quenchCancelBubble);
      },
      set(value) {
        this._quenchCancelBubble = Boolean(value);
      },
      configurable: true
    });
  } catch (_) {}
  Event.prototype.stopPropagation ||= function () {
    this.cancelBubble = true;
  };
}
if (globalThis.CustomEvent) {
  CustomEvent.NONE ||= 0;
  CustomEvent.CAPTURING_PHASE ||= 1;
  CustomEvent.AT_TARGET ||= 2;
  CustomEvent.BUBBLING_PHASE ||= 3;
}
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
