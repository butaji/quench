const __quenchEventListenersOriginalAdd =
  EventTarget.prototype.addEventListener;
EventTarget.prototype.addEventListener = function (name, listener, options) {
  const current = this._listeners?.[name] || [];
  if (!current.includes(listener)) {
    __quenchEventListenersOriginalAdd.call(this, name, listener, options);
  }
};
const __quenchEventEmitter = globalThis.__nodeEventEmitter;
__quenchEventEmitter.prototype.listenerCount = function (event, listener) {
  const value = this._events[event];
  const values = value === undefined
    ? []
    : Array.isArray(value)
    ? value
    : [value];
  return listener === undefined
    ? values.length
    : values.filter((item) => item === listener || item.listener === listener)
      .length;
};
const __quenchEventListenersOriginalRequire = globalThis.require;
const __quenchEventListenersModule = __quenchEventListenersOriginalRequire(
  "events",
);
__quenchEventListenersModule.listenerCount = (emitter, event, listener) => {
  if (
    emitter && emitter._listeners &&
    typeof emitter.addEventListener === "function"
  ) {
    const values = emitter._listeners[event] || [];
    return listener === undefined
      ? values.length
      : values.filter((item) => item === listener).length;
  }
  if (!emitter || typeof emitter.listenerCount !== "function") {
    const error = new TypeError(
      "The emitter argument must be an instance of EventEmitter [ERR_INVALID_ARG_TYPE]",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  return emitter.listenerCount(event, listener);
};
__quenchEventListenersModule.addAbortListener = (signal, listener) => {
  if (!signal || typeof signal.addEventListener !== "function") {
    const error = new TypeError(
      "The signal argument must be an AbortSignal [ERR_INVALID_ARG_TYPE]",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof listener !== "function") {
    const error = new TypeError(
      "The listener argument must be a function [ERR_INVALID_ARG_TYPE]",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  Symbol.dispose ||= Symbol("dispose");
  let active = true;
  const onAbort = () => {
    if (!active) return;
    active = false;
    signal.removeEventListener("abort", onAbort);
    listener();
  };
  signal.addEventListener("abort", onAbort, { once: true });
  if (signal.aborted) queueMicrotask(onAbort);
  return {
    [Symbol.dispose]() {
      if (!active) return;
      active = false;
      signal.removeEventListener("abort", onAbort);
    },
  };
};
__quenchEventListenersModule.getEventListeners = (target, event) => {
  if (target && target._events) {
    return [...(target._events[event] || [])].map(
      (item) => item.listener || item,
    );
  }
  if (target && target._listeners) {
    return [...(target._listeners[event] || [])]
      .map((item) => item.listener || item)
      .filter(
        (listener, index, listeners) => listeners.indexOf(listener) === index,
      );
  }
  const error = new TypeError(
    "The emitter argument must be an instance of EventEmitter or EventTarget [ERR_INVALID_ARG_TYPE]",
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
