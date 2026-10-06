//! Guest bootstrap source shared by the Node host adapters.

pub const ABORT: &str = quench_js_check::checked_js!(r#"
(() => {
  const signalStates = new WeakMap();
  const controllerSignals = new WeakMap();
  const constructorToken = Symbol("AbortSignal constructor");

  const invalidReceiver = (name) => {
    throw new TypeError(`AbortSignal.prototype.${name} called on an incompatible receiver`);
  };

  const signalState = (signal, name) => {
    const state = signalStates.get(signal);
    if (!state) invalidReceiver(name);
    return state;
  };

  const makeAbortReason = () => {
    const error = new Error("This operation was aborted");
    error.name = "AbortError";
    return error;
  };

  const invokeListener = (listener, signal, event) => {
    if (typeof listener === "function") {
      Reflect.apply(listener, signal, [event]);
    } else if (listener && typeof listener.handleEvent === "function") {
      Reflect.apply(listener.handleEvent, listener, [event]);
    }
  };

  const abortEvent = (signal) => ({
      type: "abort",
      target: signal,
      currentTarget: signal,
      defaultPrevented: false,
      preventDefault() { this.defaultPrevented = true; },
      stopPropagation() {},
      stopImmediatePropagation() { this.__stopped = true; },
    });

  const dispatchListeners = (signal, state, event) => {
    for (const entry of state.listeners.slice()) {
      if (event.__stopped) break;
      if (entry.once) removeAbortListener(state, entry.listener);
      invokeListener(entry.listener, signal, event);
    }
    if (!event.__stopped && typeof state.onabort === "function") {
      invokeListener(state.onabort, signal, event);
    }
  };

  const dispatchAbort = (signal, state) => {
    dispatchListeners(signal, state, abortEvent(signal));
  };

  const removeAbortListener = (state, listener) => {
    state.listeners = state.listeners.filter((entry) => entry.listener !== listener);
  };

  class AbortSignal {
    constructor(token) {
      if (token !== constructorToken) {
        throw Object.assign(new TypeError("Illegal constructor"), {
          code: "ERR_ILLEGAL_CONSTRUCTOR",
        });
      }
      signalStates.set(this, {
        aborted: false,
        reason: undefined,
        onabort: null,
        listeners: [],
      });
    }

    addEventListener(type, listener, options = undefined) {
      const state = signalState(this, "addEventListener");
      if (type !== "abort" || listener == null) return;
      const callback = typeof listener === "function" || typeof listener.handleEvent === "function";
      if (!callback || state.listeners.some((entry) => entry.listener === listener)) return;
      state.listeners.push({ listener, once: Boolean(options?.once) });
    }

    removeEventListener(type, listener) {
      const state = signalState(this, "removeEventListener");
      if (type === "abort") removeAbortListener(state, listener);
    }

    dispatchEvent(event) {
      const state = signalState(this, "dispatchEvent");
      if (!event || event.type !== "abort") return true;
      const dispatched = abortEvent(this);
      dispatchListeners(this, state, dispatched);
      return !dispatched.defaultPrevented;
    }

    throwIfAborted() {
      const state = signalState(this, "throwIfAborted");
      if (state.aborted) throw state.reason;
    }

    get aborted() {
      return signalState(this, "aborted").aborted;
    }

    get reason() {
      return signalState(this, "reason").reason;
    }

    get onabort() {
      return signalState(this, "onabort").onabort;
    }

    set onabort(callback) {
      signalState(this, "onabort").onabort = callback;
    }

    static abort(reason = makeAbortReason()) {
      const controller = new AbortController();
      controller.abort(reason);
      return controller.signal;
    }

    static any(signals) {
      if (signals == null || typeof signals[Symbol.iterator] !== "function") {
        throw new TypeError("The \"signals\" argument must be an iterable of AbortSignals");
      }
      const controller = new AbortController();
      for (const signal of signals) {
        const state = signalStates.get(signal);
        if (!state) throw new TypeError("Each signal must be an AbortSignal");
        if (state.aborted) {
          controller.abort(state.reason);
          break;
        }
        signal.addEventListener("abort", () => controller.abort(signal.reason), { once: true });
      }
      return controller.signal;
    }

    static timeout(milliseconds) {
      if (typeof globalThis.setTimeout !== "function") {
        throw new Error("AbortSignal.timeout requires timers");
      }
      const controller = new AbortController();
      globalThis.setTimeout(() => {
        const error = new Error("The operation was aborted due to timeout");
        error.name = "TimeoutError";
        controller.abort(error);
      }, milliseconds);
      return controller.signal;
    }
  }

  class AbortController {
    constructor() {
      controllerSignals.set(this, new AbortSignal(constructorToken));
    }

    get signal() {
      const signal = controllerSignals.get(this);
      if (!signal) throw new TypeError("AbortController.prototype.signal called on an incompatible receiver");
      return signal;
    }

    abort(reason = makeAbortReason()) {
      const signal = this.signal;
      const state = signalState(signal, "abort");
      if (state.aborted) return;
      state.aborted = true;
      state.reason = reason;
      dispatchAbort(signal, state);
    }
  }

  Object.defineProperty(AbortSignal.prototype, Symbol.toStringTag, { value: "AbortSignal" });
  Object.defineProperty(AbortController.prototype, Symbol.toStringTag, { value: "AbortController" });
  Object.defineProperty(globalThis, "AbortSignal", {
    value: AbortSignal,
    writable: true,
    configurable: true,
  });
  Object.defineProperty(globalThis, "AbortController", {
    value: AbortController,
    writable: true,
    configurable: true,
  });
  Object.defineProperty(globalThis, "__quenchAbortListenerCount", {
    value: (signal, event) => {
      const state = signalStates.get(signal);
      return event === "abort" && state ? state.listeners.length : 0;
    },
    configurable: true,
  });
})();
"#);

pub const EVENT_TARGET: &str = quench_js_check::checked_js!(r#"
if (globalThis.Event === undefined) Object.defineProperty(globalThis, "Event", {
  value: class Event {
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
  },
  writable: true,
  configurable: true,
});

if (globalThis.EventTarget === undefined) Object.defineProperty(globalThis, "EventTarget", {
  value: class EventTarget {
  constructor() {
    this._listeners = Object.create(null);
  }
  addEventListener(type, listener, options = undefined) {
    if (listener == null || typeof listener !== "function") return;
    const records = this._listeners[type] ||= [];
    if (records.some((record) => record.listener === listener)) return;
    const once = Boolean(options && typeof options === "object" && options.once);
    records.push({ listener, once });
  }
  removeEventListener(type, listener) {
    const records = this._listeners[type];
    if (records) this._listeners[type] = records.filter((record) => record.listener !== listener);
  }
  dispatchEvent(event) {
    for (const record of (this._listeners[event.type] || []).slice()) {
      if (!(this._listeners[event.type] || []).includes(record)) continue;
      if (record.once) this.removeEventListener(event.type, record.listener);
      record.listener.call(this, event);
      if (event._quenchImmediatePropagationStopped) break;
    }
    return !event.defaultPrevented;
  }
  },
  writable: true,
  configurable: true,
});
"#);
