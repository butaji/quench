//! Guest bootstrap source shared by the Node host adapters.

pub const ABORT: &str = quench_js_check::checked_js!(
    r#"
(() => {
  const signalStates = new WeakMap();
  const controllerSignals = new WeakMap();
  const dependantSignals = Symbol("kDependantSignals");
  const dependantFinalizer = new FinalizationRegistry((entry) => {
    const source = entry.source.deref();
    const dependant = entry.dependant.deref();
    if (source && dependant) signalStates.get(source).children.delete(entry.dependant);
  });
  const constructorToken = Symbol("AbortSignal constructor");

  const invalidReceiver = (name) => {
    throw new TypeError(`AbortSignal.prototype.${name} called on an incompatible receiver`);
  };

  if (typeof globalThis.DOMException !== "function") {
    const codes = { IndexSizeError: 1, HierarchyRequestError: 3, WrongDocumentError: 4,
      InvalidCharacterError: 5, NoModificationAllowedError: 7, NotFoundError: 8,
      NotSupportedError: 9, InUseAttributeError: 10, InvalidStateError: 11,
      SyntaxError: 12, InvalidModificationError: 13, NamespaceError: 14,
      TypeMismatchError: 17, SecurityError: 18, NetworkError: 19, AbortError: 20,
      URLMismatchError: 21, QuotaExceededError: 22, TimeoutError: 23,
      InvalidNodeTypeError: 24, DataCloneError: 25 };
    Object.defineProperty(globalThis, "DOMException", {
      configurable: true,
      value: class DOMException extends Error {
        constructor(message = "", name = "Error") {
          super(message);
          this.name = name;
          this.code = codes[name] || 0;
        }
      },
    });
  }

  const signalState = (signal, name) => {
    const state = signalStates.get(signal);
    if (!state) invalidReceiver(name);
    return state;
  };

  const makeAbortReason = () => {
    return new DOMException("This operation was aborted", "AbortError");
  };

  const invokeListener = (listener, signal, event) => {
    if (typeof listener === "function") {
      Reflect.apply(listener, signal, [event]);
    } else if (listener && typeof listener.handleEvent === "function") {
      Reflect.apply(listener.handleEvent, listener, [event]);
    }
  };

  const abortEvent = (signal) => {
    const event = new Event("abort");
    event.target = signal;
    event.currentTarget = signal;
    event._quenchIsTrusted = true;
    event.__stopped = false;
    return event;
  };

  const dispatchListeners = (signal, state, event) => {
    for (const entry of state.listeners.slice()) {
      if (event.__stopped) break;
      if (entry.once) removeAbortListener(signal, state, entry.listener);
      invokeListener(entry.listener, signal, event);
    }
    if (!event.__stopped && typeof state.onabort === "function") {
      invokeListener(state.onabort, signal, event);
    }
  };

  const abortSignal = (signal, reason) => {
    const pending = [{ signal, reason }];
    const aborted = [];
    for (let index = 0; index < pending.length; index++) {
      const item = pending[index];
      const state = signalStates.get(item.signal);
      if (state.aborted) continue;
      state.aborted = true;
      state.reason = item.reason;
      item.signal.aborted = true;
      aborted.push(item.signal);
      for (const dependent of state.children) {
        const value = dependent.deref();
        if (value) pending.push({ signal: value, reason: item.reason });
      }
    }
    for (const value of aborted) {
      const state = signalStates.get(value);
      settleDependents(value, state);
      dispatchListeners(value, state, abortEvent(value));
    }
  };

  const refreshDependents = (signal, state) => {
    const observed = state.listeners.length > 0 || state.onabort !== null;
    for (const source of state.parents) {
      const dependents = signalStates.get(source).dependents;
      if (observed) dependents.add(signal);
      else dependents.delete(signal);
    }
  };

  const settleDependents = (signal, state) => {
    dependantFinalizer.unregister(signal);
    for (const source of state.parents) {
      const sourceState = signalStates.get(source);
      sourceState.children.delete(state.childReferences.get(source));
      sourceState.dependents.delete(signal);
    }
  };

  const removeAbortListener = (signal, state, listener) => {
    state.listeners = state.listeners.filter((entry) => entry.listener !== listener);
    refreshDependents(signal, state);
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
        dependents: new Set(),
        children: new Set(),
        parents: [],
        childReferences: new Map(),
      });
      Object.defineProperty(this, dependantSignals, {
        configurable: false,
        enumerable: false,
        value: signalStates.get(this).dependents,
      });
      Object.defineProperty(this, "aborted", {
        configurable: true,
        enumerable: true,
        writable: true,
        value: false,
      });
    }

    addEventListener(type, listener, options = undefined) {
      const state = signalState(this, "addEventListener");
      if (type !== "abort" || listener == null) return;
      const callback = typeof listener === "function" || typeof listener.handleEvent === "function";
      if (!callback || state.listeners.some((entry) => entry.listener === listener)) return;
      state.listeners.push({ listener, once: Boolean(options?.once) });
      refreshDependents(this, state);
    }

    removeEventListener(type, listener) {
      const state = signalState(this, "removeEventListener");
      if (type === "abort") removeAbortListener(this, state, listener);
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
      const state = signalState(this, "onabort");
      state.onabort = callback;
      refreshDependents(this, state);
    }

    static abort(reason = makeAbortReason()) {
      const controller = new AbortController();
      controller.abort(reason);
      return controller.signal;
    }

    static any(signals) {
      const invalidArgument = (message) => Object.assign(new TypeError(message), {
        code: "ERR_INVALID_ARG_TYPE",
      });
      if (signals == null || typeof signals[Symbol.iterator] !== "function") {
        throw invalidArgument('The "signals" argument must be an instance of Array');
      }
      const values = [];
      let index = 0;
      for (const signal of signals) {
        if (!signalStates.has(signal)) {
          throw invalidArgument(`signals[${index}] is not of type AbortSignal.`);
        }
        values.push(signal);
        index++;
      }
      const controller = new AbortController();
      const aborted = values.find((signal) => signalStates.get(signal).aborted);
      if (aborted) {
        controller.abort(aborted.reason);
      } else {
        const state = signalStates.get(controller.signal);
        const sources = new Set();
        for (const signal of values) {
          sources.add(signal);
          for (const source of signalStates.get(signal).parents) sources.add(source);
        }
        for (const signal of sources) {
          const sourceState = signalStates.get(signal);
          const reference = new WeakRef(controller.signal);
          sourceState.children.add(reference);
          state.parents.push(signal);
          state.childReferences.set(signal, reference);
          dependantFinalizer.register(controller.signal, {
            source: new WeakRef(signal),
            dependant: reference,
          }, controller.signal);
        }
      }
      return controller.signal;
    }

    static timeout(milliseconds) {
      if (typeof globalThis.setTimeout !== "function") {
        throw new Error("AbortSignal.timeout requires timers");
      }
      const controller = new AbortController();
      globalThis.setTimeout(() => {
        const error = new DOMException("The operation was aborted due to timeout", "TimeoutError");
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
      abortSignal(signal, reason);
    }
  }

  const inspectCustom = Symbol.for("nodejs.util.inspect.custom");
  Object.defineProperty(AbortSignal.prototype, inspectCustom, {
    configurable: true,
    value(depth) {
      const state = signalState(this, "[nodejs.util.inspect.custom]");
      if (depth < 0) return "[AbortSignal]";
      return `AbortSignal { aborted: ${state.aborted} }`;
    },
  });
  Object.defineProperty(AbortController.prototype, inspectCustom, {
    configurable: true,
    value(depth, options) {
      const signal = controllerSignals.get(this);
      if (!signal) throw new TypeError("AbortController.prototype.signal called on an incompatible receiver");
      if (depth < 0 || options?.depth === 1) return "AbortController { signal: [AbortSignal] }";
      const state = signalState(signal, "[nodejs.util.inspect.custom]");
      return `AbortController { signal: AbortSignal { aborted: ${state.aborted} } }`;
    },
  });

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
"#
);

pub const EVENT_TARGET: &str = quench_js_check::checked_js!(
    r#"
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

if (typeof globalThis.Event === "function" &&
    !Object.getOwnPropertyDescriptor(Event.prototype, "isTrusted")?.get) {
  Object.defineProperty(Event.prototype, "isTrusted", {
    configurable: true,
    enumerable: true,
    get() { return this._quenchIsTrusted === true; },
  });
}

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

Object.defineProperty(globalThis, "MessageEvent", {
  value: (() => {
    const brand = new WeakSet();
    const assertBrand = (value) => { if (!brand.has(value)) throw new TypeError("Illegal invocation"); };
    return class MessageEvent extends Event {
      constructor(type, init = {}) {
        super(type, init);
        brand.add(this);
        this._data = init.data;
        this._origin = init.origin === undefined ? "" : String(init.origin);
        this._lastEventId = init.lastEventId === undefined ? "" : String(init.lastEventId);
        this._source = init.source ?? null;
        this._ports = init.ports === undefined ? [] : [...init.ports];
      }
      get data() { assertBrand(this); return this._data; }
      get origin() { assertBrand(this); return this._origin; }
      get lastEventId() { assertBrand(this); return this._lastEventId; }
      get source() { assertBrand(this); return this._source; }
      get ports() { assertBrand(this); return this._ports; }
    };
  })(),
  writable: true,
  configurable: true,
});

if (globalThis.MessagePort === undefined) Object.defineProperty(globalThis, "MessagePort", {
  value: class MessagePort extends EventTarget {
    constructor() {
      super();
      this.onmessage = null;
      this._peer = null;
      this._closed = false;
      this._refed = false;
      this._nodeListeners = new Map();
      globalThis["\0quench:async_hooks:emit_init"]?.(this, "MESSAGEPORT");
    }
    start() {}
    close(callback) {
      this._closed = true;
      const peer = this._peer;
      queueMicrotask(() => {
        this._refed = false;
        if (peer) peer._refed = false;
        this.dispatchEvent(new Event("close"));
        for (const listener of this._nodeListeners.get("close") || []) listener();
        if (peer && !peer._closed) {
          peer.dispatchEvent(new Event("close"));
          for (const listener of peer._nodeListeners.get("close") || []) listener();
        }
        if (typeof callback === "function") callback();
      });
    }
    ref() { this._refed = true; return this; }
    unref() { this._refed = false; return this; }
    hasRef() { return this._refed; }
    on(name, listener) {
      const listeners = this._nodeListeners.get(name) || [];
      listeners.push(listener);
      this._nodeListeners.set(name, listeners);
      if (name === "message") this._refed = true;
      return this;
    }
    addListener(name, listener) { return this.on(name, listener); }
    once(name, listener) {
      const wrapped = (...args) => { this.removeListener(name, wrapped); listener(...args); };
      return this.on(name, wrapped);
    }
    removeListener(name, listener) {
      const listeners = this._nodeListeners.get(name) || [];
      this._nodeListeners.set(name, listeners.filter((item) => item !== listener));
      return this;
    }
    off(name, listener) { return this.removeListener(name, listener); }
    postMessage(value, transferList) {
      if (this._closed || !this._peer || this._peer._closed) return;
      let data;
      if (value && value.constructor?.name === "BlockList" && typeof value.toJSON === "function") {
        data = Object.create(Object.getPrototypeOf(value));
        data._rules = value._rules;
      }
      if (data === undefined) {
        if (typeof globalThis.structuredClone !== "function") throw new DOMException("Value could not be cloned.", "DataCloneError");
        const transfers = transferList?.transfer ?? transferList;
        data = globalThis.structuredClone(value, transfers === undefined ? undefined : { transfer: [...transfers] });
      }
      const peer = this._peer;
      queueMicrotask(() => {
        if (peer._closed) return;
        const event = new MessageEvent("message", { data, ports: [] });
        peer.dispatchEvent(event);
        for (const listener of peer._nodeListeners.get("message") || []) listener(data);
        if (typeof peer.onmessage === "function") peer.onmessage.call(peer, event);
      });
    }
    emit(name, ...args) {
      for (const listener of this._nodeListeners.get(name) || []) listener(...args);
      return this.dispatchEvent(Object.assign(new Event(name), { detail: args[0] }));
    }
  },
  writable: true,
  configurable: true,
});

if (globalThis.MessageChannel === undefined) Object.defineProperty(globalThis, "MessageChannel", {
  value: class MessageChannel {
    constructor() {
      this.port1 = new MessagePort();
      this.port2 = new MessagePort();
      this.port1._peer = this.port2;
      this.port2._peer = this.port1;
    }
  },
  writable: true,
  configurable: true,
});
"#
);
