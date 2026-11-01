//! Guest bootstrap source shared by the Node host adapters.

pub const ABORT: &str = quench_js_check::checked_js!(
    r#"
((maxKey, warnedKey, maxDefault, addNative, dispatchNative, getHandlerNative, setHandlerNative, removeNative, anyConvertNative, linkNative, refreshNative, settleNative, timeoutNative) => {
  const signalStates = new WeakMap();
  let trustEventKey;
  const controllerSignals = new WeakMap();
  const dependantSignals = Symbol("kDependantSignals");
  const setSize = Object.getOwnPropertyDescriptor(Set.prototype, "size").get;
  class DependentSignalSet extends Set {
    get size() {
      for (const reference of this) {
        if (reference.deref() === undefined) this.delete(reference);
      }
      return setSize.call(this);
    }
  }
  const constructorToken = Symbol("AbortSignal constructor");
  const constructSignal = () => new AbortSignal(constructorToken);

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

  const abortEvent = (signal) => {
    const options = trustEventKey === undefined ? undefined : { [trustEventKey]: true };
    const event = new Event("abort", options);
    event._quenchIsTrusted = true;
    event.__stopped = false;
    return event;
  };

  const dispatchListeners = (signal, state, event) =>
    dispatchNative(signal, state, event, removeAbortListener);

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

  const refreshDependents = (signal, state) => refreshNative(signal, state);
  const settleDependents = (signal, state) => settleNative(signal, state);

  const removeAbortListener = (signal, state, listener) =>
    removeNative(signal, state, listener, refreshDependents);

  class AbortSignal {
    constructor(token) {
      if (token !== constructorToken) {
        throw Object.assign(new TypeError("Illegal constructor"), {
          code: "ERR_ILLEGAL_CONSTRUCTOR",
        });
      }
      this[maxKey] = maxDefault;
      this[warnedKey] = false;
      signalStates.set(this, {
        aborted: false,
        reason: undefined,
        onabortWrapper: undefined,
        listeners: [],
        listenerCount: 0,
        dependents: new Set(),
        children: new DependentSignalSet(),
        parents: [],
        composite: false,
        eventListeners: new Map(),
      });
      Object.defineProperty(this, dependantSignals, {
        configurable: false,
        enumerable: false,
        value: signalStates.get(this).children,
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
      if (type !== "abort") {
        const eventType = String(type);
        if (listener == null || (typeof listener !== "function" && typeof listener !== "object")) return;
        const records = state.eventListeners.get(eventType) || [];
        if (records.some((record) => record.listener === listener)) return;
        const record = { listener, once: Boolean(options && typeof options === "object" && options.once) };
        records.push(record);
        state.eventListeners.set(eventType, records);
        const maximum = this[maxKey];
        if (maximum > 0 && records.length > maximum && !this[warnedKey]) {
          this[warnedKey] = true;
          const warning = new Error(`Possible EventTarget memory leak detected. ${records.length} ${eventType} listeners added to [AbortSignal]. MaxListeners is ${maximum}. Use events.setMaxListeners() to increase limit`);
          warning.name = "MaxListenersExceededWarning";
          warning.target = this;
          warning.type = eventType;
          warning.count = records.length;
          globalThis.process?.emitWarning?.(warning);
        }
        return;
      }
      return addNative(this, state, type, listener, options, refreshDependents);
    }

    removeEventListener(type, listener) {
      const state = signalState(this, "removeEventListener");
      if (type === "abort") {
        removeAbortListener(this, state, listener);
      } else {
        const eventType = String(type);
        const records = state.eventListeners.get(eventType);
        if (records) state.eventListeners.set(eventType, records.filter((record) => record.listener !== listener));
      }
    }

    dispatchEvent(event) {
      const state = signalState(this, "dispatchEvent");
      if (!event || event.type !== "abort") {
        const records = state.eventListeners.get(String(event?.type)) || [];
        for (const record of records.slice()) {
          if (!records.includes(record)) continue;
          if (record.once) this.removeEventListener(event.type, record.listener);
          if (typeof record.listener === "function") record.listener.call(this, event);
          else if (typeof record.listener.handleEvent === "function") record.listener.handleEvent.call(record.listener, event);
        }
        return !event?.defaultPrevented;
      }
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
      return getHandlerNative(signalState(this, "onabort"));
    }

    set onabort(callback) {
      const state = signalState(this, "onabort");
      return setHandlerNative(this, state, callback, refreshDependents);
    }

    static abort(reason = makeAbortReason()) {
      const controller = new AbortController();
      controller.abort(reason);
      return controller.signal;
    }

    static any(signals) {
      const values = anyConvertNative(signals);
      const signal = new AbortSignal(constructorToken);
      linkNative(signal, values);
      return signal;
    }

    static timeout(milliseconds) {
      return timeoutNative(milliseconds, constructSignal);
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
  return [(signal) => signalStates.get(signal), signalStates, abortSignal,
    (key) => { trustEventKey = key; }];
})
"#
);

pub const EVENT_TARGET: &str = quench_js_check::checked_js!(
    r#"(recordsKey, maxKey, warnedKey, maxDefault, initializeEvent) => {
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
    initializeEvent(this);
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
    this[recordsKey] = Object.create(null);
    this[maxKey] = maxDefault();
    this[warnedKey] = false;
  }
  addEventListener(type, listener, options = undefined) {
    if (arguments.length < 2) {
      const error = new TypeError('The "type" and "listener" arguments must be specified');
      error.code = "ERR_MISSING_ARGS";
      throw error;
    }
    if (typeof type === "symbol") throw new TypeError("Cannot convert a Symbol value to a string");
    if (listener == null) return;
    if (typeof listener !== "function" && typeof listener !== "object") {
      const received = typeof listener === "string"
        ? `type string ('${listener}')`
        : `type ${typeof listener} (${typeof listener === "bigint" ? `${listener}n` : String(listener)})`;
      const error = new TypeError(`The "listener" argument must be an instance of EventListener. Received ${received}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const records = this[recordsKey][type] ||= [];
    for (let i = records.length - 1; i >= 0; i--) {
      const current = records[i].weak ? records[i].listener.deref() : records[i].listener;
      if (current === undefined) records.splice(i, 1);
      else if (current === listener) return;
    }
    const once = Boolean(options && typeof options === "object" && options.once);
    const weakSymbol = options && typeof options === "object" &&
      Object.getOwnPropertySymbols(options).find((symbol) => symbol.description === "kWeak");
    const weak = Boolean(weakSymbol && options[weakSymbol]);
    records.push({ listener: weak ? new WeakRef(listener) : listener, weak, once });
    const maximum = this[maxKey];
    if (maximum > 0 && records.length > maximum && !this[warnedKey]) {
      this[warnedKey] = true;
      const target = this.constructor?.name === "MessagePort" ? "[MessagePort]" : "EventTarget";
      const warning = new Error(`Possible EventTarget memory leak detected. ${records.length} ${type} listeners added to ${target}. MaxListeners is ${maximum}. Use events.setMaxListeners() to increase limit`);
      warning.name = "MaxListenersExceededWarning";
      warning.target = this;
      warning.type = type;
      warning.count = records.length;
      globalThis.process?.emitWarning?.(warning);
    }
  }
  removeEventListener(type, listener) {
    if (arguments.length < 2) {
      const error = new TypeError('The "type" and "listener" arguments must be specified');
      error.code = "ERR_MISSING_ARGS";
      throw error;
    }
    if (typeof type === "symbol") throw new TypeError("Cannot convert a Symbol value to a string");
    const records = this[recordsKey][type];
    if (records) this[recordsKey][type] = records.filter((record) =>
      record.listener !== listener &&
      (record.weak ? record.listener.deref() : record.listener) !== listener);
  }
  },
  writable: true,
  configurable: true,
});

if (typeof globalThis.EventTarget === "function" &&
    !Object.prototype.hasOwnProperty.call(EventTarget.prototype, Symbol.toStringTag)) {
  Object.defineProperty(EventTarget.prototype, Symbol.toStringTag, {
    configurable: true,
    value: "EventTarget",
  });
}

Object.defineProperty(globalThis, "MessageEvent", {
  value: (() => {
    const brand = new WeakSet();
    const assertBrand = (value) => { if (!brand.has(value)) throw new TypeError("Illegal invocation"); };
    return class MessageEvent extends Event {
      constructor(type, init = {}) {
        super(type, init);
        brand.add(this);
        const formatArgument = (value) => {
          if (value !== null && typeof value === "object") {
            try {
              return JSON.stringify(value) ?? String(value);
            } catch {}
          }
          return String(value);
        };
        this._data = init.data === undefined ? null : init.data;
        this._origin = init.origin === undefined ? "" : String(init.origin);
        this._lastEventId = init.lastEventId === undefined ? "" : String(init.lastEventId);
        const source = init.source;
        if (source !== undefined && source !== null &&
            !(source instanceof MessagePort || typeof source.postMessage === "function" ||
              typeof source.start === "function")) {
          throw new TypeError(`MessageEvent constructor: Expected eventInitDict.source ("${formatArgument(source)}") to be an instance of MessagePort.`);
        }
        this._source = source === undefined ? null : source;
        if (init.ports === undefined) {
          this._ports = [];
        } else {
          const ports = init.ports;
          if (ports === null || typeof ports[Symbol.iterator] !== "function") {
            throw new TypeError(`MessageEvent constructor: eventInitDict.ports (${formatArgument(ports)}) is not iterable.`);
          }
          this._ports = [...ports];
          const invalidPort = this._ports.findIndex((port) =>
            !(port instanceof MessagePort || typeof port?.postMessage === "function"));
          if (invalidPort !== -1) {
            const port = this._ports[invalidPort];
            throw new TypeError(`MessageEvent constructor: Expected eventInitDict.ports[${invalidPort}] ("${formatArgument(port)}") to be an instance of MessagePort.`);
          }
        }
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

}
"#
);
