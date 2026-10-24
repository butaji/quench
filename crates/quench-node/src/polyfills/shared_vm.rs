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
    event.target = signal;
    event.currentTarget = signal;
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
    r#"(recordsKey, maxKey, warnedKey, maxDefault, targetKey, dispatchingKey) => {
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
  dispatchEvent(event) {
    if (arguments.length === 0) {
      const error = new TypeError('The "event" argument must be specified');
      error.code = "ERR_MISSING_ARGS";
      throw error;
    }
    if (!(event instanceof Event)) {
      const received = event === undefined || event === null
        ? String(event)
        : typeof event === "string"
          ? `type string ('${event}')`
          : typeof event === "object"
            ? `an instance of ${event.constructor?.name || "Object"}`
            : typeof event === "function"
              ? `function ${event.name}`
              : typeof event === "symbol"
                ? `type symbol (${String(event)})`
                : `type ${typeof event} (${typeof event === "bigint" ? `${event}n` : String(event)})`;
      const error = new TypeError(`The "event" argument must be an instance of Event. Received ${received}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    event[targetKey] = this;
    event[dispatchingKey] = true;
    try {
      for (const record of (this[recordsKey][event.type] || []).slice()) {
        if (!(this[recordsKey][event.type] || []).includes(record)) continue;
        const listener = record.weak ? record.listener.deref() : record.listener;
        if (listener === undefined) {
          this.removeEventListener(event.type, record.listener);
          continue;
        }
        if (record.once) this.removeEventListener(event.type, record.listener);
        try {
          let result;
          if (typeof listener === "function") {
            result = listener.call(this, event);
          } else if (typeof listener.handleEvent === "function") {
            result = listener.handleEvent.call(listener, event);
          }
          if (result !== undefined && result !== null && typeof result.then === "function") {
            result.then(undefined, (error) => globalThis.process?.nextTick?.(() => { throw error; }));
          }
        } catch (error) {
          globalThis.process?.nextTick?.(() => { throw error; });
        }
        if (event._quenchImmediatePropagationStopped) break;
      }
    } finally {
      event[dispatchingKey] = false;
    }
    return !event.defaultPrevented;
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

const __quenchMessagePortToken = Symbol("quench internal MessagePort construction");
if (globalThis.MessagePort === undefined) {
  let __quenchTransferSequence = 0;
  const __quenchMessagePortImplementation = class extends EventTarget {
    constructor(token) {
      if (token !== __quenchMessagePortToken) {
        const error = new TypeError("Illegal constructor");
        error.code = "ERR_CONSTRUCT_CALL_INVALID";
        throw error;
      }
      super();
      // Keep implementation state out of Object.keys(). Native MessagePort
      // instances expose only user-added enumerable properties, which is
      // significant for DefaultSerializer's host-object fallback.
      Object.defineProperties(this, {
        _onmessage: { value: null, writable: true, configurable: true },
        _onmessageerror: { value: null, writable: true, configurable: true },
        _peer: { value: null, writable: true, configurable: true },
        _closed: { value: false, writable: true, configurable: true },
        _refed: { value: false, writable: true, configurable: true },
        _queue: { value: [], writable: true, configurable: true },
        _dispatchScheduled: { value: false, writable: true, configurable: true },
        _nodeListeners: { value: new Map(), writable: true, configurable: true },
        _eventMessageListeners: { value: new Set(), writable: true, configurable: true },
        _eventMessageOnceListeners: { value: new Set(), writable: true, configurable: true },
      });
      globalThis["\0quench:async_hooks:emit_init"]?.(this, "MESSAGEPORT");
    }
    start() { this.__scheduleQueuedMessages(); }
    close(callback) {
      this._closed = true;
      const peer = this._peer;
      queueMicrotask(() => {
        this._refed = false;
        if (peer) peer._refed = false;
        this.dispatchEvent(new Event("close"));
        for (const listener of this._nodeListeners.get("close") || []) listener();
        if (peer && !peer._closed) {
          peer._closed = true;
          peer.dispatchEvent(new Event("close"));
          for (const listener of peer._nodeListeners.get("close") || []) listener();
        }
        if (typeof callback === "function") callback();
      });
    }
    ref() { this._refed = true; }
    unref() { this._refed = false; }
    hasRef() { return this._refed; }
    [Symbol.for("nodejs.util.inspect.custom")]() {
      return `MessagePort { active: ${!this._closed}, refed: ${this._refed} }`;
    }
    on(name, listener) {
      const listeners = this._nodeListeners.get(name) || [];
      listeners.push(listener);
      this._nodeListeners.set(name, listeners);
      if (name === "message") {
        this._refed = true;
        this.__scheduleQueuedMessages();
      }
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
      let data;
      let messagePorts = [];
      const portPlaceholders = new Map();
      if (value && value.constructor?.name === "BlockList" && typeof value.toJSON === "function") {
        data = Object.create(Object.getPrototypeOf(value));
        data._rules = value._rules;
      }
      if (data === undefined) {
        if (typeof value === "function") {
          const source = Function.prototype.toString.call(value);
          throw new DOMException(`${source} could not be cloned.`, "DataCloneError");
        }
        if (value && typeof value === "object" && value.__quenchJSStream === true) {
          throw new DOMException("Cannot clone object of unsupported type.", "DataCloneError");
        }
        if (typeof globalThis.structuredClone !== "function") throw new DOMException("Value could not be cloned.", "DataCloneError");
        const hasTransferOption = transferList !== null &&
          typeof transferList === "object" && !Array.isArray(transferList) &&
          typeof transferList[Symbol.iterator] !== "function";
        const transfers = hasTransferOption
          ? transferList.transfer
          : transferList === null ? undefined : transferList;
        const argument = hasTransferOption ? "options.transfer" : "transferList";
        if (transfers !== undefined &&
            (transfers === null || typeof transfers !== "object" ||
             typeof transfers[Symbol.iterator] !== "function")) {
          throw Object.assign(
            new TypeError(`Optional ${argument} argument must be an iterable`),
            { code: "ERR_INVALID_ARG_TYPE" },
          );
        }
        let transferValues;
        if (transfers !== undefined) {
          const iterator = transfers[Symbol.iterator]();
          if (iterator === null || typeof iterator !== "object" ||
              typeof iterator.next !== "function") {
            throw Object.assign(
              new TypeError(`Optional ${argument} argument must be an iterable`),
              { code: "ERR_INVALID_ARG_TYPE" },
            );
          }
          transferValues = [];
          while (true) {
            const result = iterator.next();
            if (result === null || typeof result !== "object") {
              throw Object.assign(
                new TypeError(`Optional ${argument} argument must be an iterable`),
                { code: "ERR_INVALID_ARG_TYPE" },
              );
            }
            if (result.done) break;
            transferValues.push(result.value);
          }
          if (transferValues.includes(this)) {
            throw new DOMException("Transfer list contains source port", "DataCloneError");
          }
          if (transferValues.some((transfer) =>
            transfer instanceof MessagePort && transfer._closed)) {
            throw new DOMException(
              "MessagePort in transfer list is already detached",
              "DataCloneError",
            );
          }
          const seenTransfers = new Set();
          for (const transfer of transferValues) {
            if (seenTransfers.has(transfer)) {
              const type = transfer instanceof ArrayBuffer
                ? "ArrayBuffer"
                : transfer instanceof MessagePort ? "MessagePort" : "object";
              throw new DOMException(`Transfer list contains duplicate ${type}`, "DataCloneError");
            }
            seenTransfers.add(transfer);
          }
          messagePorts = transferValues.filter((transfer) => transfer instanceof MessagePort);
        }
        if (this._closed || !this._peer || this._peer._closed) return;
        const transferArrayBuffers = transferValues?.filter((transfer) => !(transfer instanceof MessagePort));
        const cloneableValue = (() => {
          if (messagePorts.length === 0) return value;
          const seen = new Map();
          const prepare = (item) => {
            if (messagePorts.includes(item)) {
              let placeholder = portPlaceholders.get(item);
              if (placeholder === undefined) {
                const token = `__quench_port_transfer_${++__quenchTransferSequence}__`;
                placeholder = Object.create(null);
                placeholder[token] = true;
                portPlaceholders.set(item, placeholder);
              }
              return placeholder;
            }
            if (item === null || typeof item !== "object") return item;
            if (seen.has(item)) return seen.get(item);
            let copy;
            if (Array.isArray(item)) copy = [];
            else if (item instanceof Map) copy = new Map();
            else if (item instanceof Set) copy = new Set();
            else {
              const prototype = Object.getPrototypeOf(item);
              if (prototype !== Object.prototype && prototype !== null) return item;
              copy = Object.create(prototype);
            }
            seen.set(item, copy);
            if (item instanceof Map) {
              for (const [key, entry] of item) copy.set(prepare(key), prepare(entry));
            } else if (item instanceof Set) {
              for (const entry of item) copy.add(prepare(entry));
            } else {
              for (const key of Object.keys(item)) copy[key] = prepare(item[key]);
            }
            return copy;
          };
          return prepare(value);
        })();
        data = globalThis.structuredClone(
          cloneableValue,
          transferArrayBuffers === undefined ? undefined : { transfer: transferArrayBuffers },
        );
        if (messagePorts.length > 0) {
          const movedPorts = new Map();
          const movedPortsList = [];
          for (const port of messagePorts) {
            const moved = new MessagePort(__quenchMessagePortToken);
            const peer = port._peer;
            moved._peer = peer;
            if (peer) peer._peer = moved;
            port._peer = null;
            port._closed = true;
            port._refed = false;
            movedPortsList.push(moved);
            const placeholder = portPlaceholders.get(port);
            if (placeholder !== undefined) movedPorts.set(Object.keys(placeholder)[0], moved);
          }
          const restored = new Map();
          const replace = (item) => {
            if (item === null || typeof item !== "object") return item;
            const marker = Object.keys(item);
            if (marker.length === 1 && item[marker[0]] === true && movedPorts.has(marker[0])) {
              return movedPorts.get(marker[0]);
            }
            if (restored.has(item)) return restored.get(item);
            if (Array.isArray(item)) {
              restored.set(item, item);
              for (let i = 0; i < item.length; i++) item[i] = replace(item[i]);
            } else if (item instanceof Map) {
              restored.set(item, item);
              for (const [key, entry] of item) {
                const nextKey = replace(key);
                const nextEntry = replace(entry);
                if (nextKey !== key) item.delete(key);
                item.set(nextKey, nextEntry);
              }
            } else if (item instanceof Set) {
              restored.set(item, item);
              const entries = [...item].map(replace);
              item.clear();
              for (const entry of entries) item.add(entry);
            } else {
              restored.set(item, item);
              for (const key of Object.keys(item)) item[key] = replace(item[key]);
            }
            return item;
          };
          data = replace(data);
          messagePorts = movedPortsList;
        }
      }
      if (messagePorts.includes(this._peer)) {
        globalThis.process?.emitWarning?.("The target port was posted to itself, and the communication channel was lost");
        return;
      }
      const peer = this._peer;
      const message = { data, ports: messagePorts, canceled: false };
      peer._queue.push(message);
      peer.__scheduleQueuedMessages();
    }
    emit(name, ...args) {
      for (const listener of this._nodeListeners.get(name) || []) listener(...args);
      if (name === "message") {
        const event = new MessageEvent("message", { data: args[0], ports: [] });
        const delivered = this.dispatchEvent(event);
        if (typeof this.onmessage === "function") {
          this.onmessage.call(this, event);
          return true;
        }
        return delivered;
      }
      return this.dispatchEvent(Object.assign(new Event(name), { detail: args[0] }));
    }
    __dispatchQueuedMessages() {
      const listeners = this._nodeListeners.get("message") || [];
      if (listeners.length === 0 && this._eventMessageListeners.size === 0 &&
          typeof this.onmessage !== "function") return;
      const message = this._queue[0];
      if (!message) return;
      if (message.canceled) {
        this._queue.shift();
        this.__scheduleQueuedMessages();
        return;
      }
      this._queue.shift();
      const event = new MessageEvent("message", {
        data: message.data,
        ports: message.ports,
      });
      this.dispatchEvent(event);
      for (const listener of this._eventMessageOnceListeners) {
        this._eventMessageListeners.delete(listener);
      }
      this._eventMessageOnceListeners.clear();
      for (const listener of this._nodeListeners.get("message") || []) {
        listener(message.data);
      }
      if (typeof this.onmessage === "function") {
        this.onmessage.call(this, event);
      }
      if (this._queue.length > 0) {
        this.__scheduleQueuedMessages();
      }
    }
    __scheduleQueuedMessages() {
      if (this._dispatchScheduled || this._queue.length === 0) return;
      this._dispatchScheduled = true;
      // MessagePort callbacks are event-loop tasks. A microtask per message
      // lets self-posting ports starve timers indefinitely, unlike Node. Use
      // a timer task here so a recursively-posted message yields to timers
      // already queued by the current callback (as in Node's poll phase).
      setTimeout(() => {
        this._dispatchScheduled = false;
        this.__dispatchQueuedMessages();
      }, 0);
    }
  };
  // Keep the public MessagePort prototype surface aligned with Node: the
  // EventEmitter helpers remain available through inheritance, while the
  // native own-property list contains only MessagePort's documented methods.
  const __quenchMessagePortPrototype = __quenchMessagePortImplementation.prototype;
  const __quenchMessagePortEventEmitterPrototype = Object.create(
    Object.getPrototypeOf(__quenchMessagePortPrototype),
  );
  for (const name of ["on", "addListener", "once", "removeListener", "off", "emit"]) {
    const descriptor = Object.getOwnPropertyDescriptor(__quenchMessagePortPrototype, name);
    Object.defineProperty(__quenchMessagePortEventEmitterPrototype, name, descriptor);
    delete __quenchMessagePortPrototype[name];
  }
  Object.defineProperties(__quenchMessagePortEventEmitterPrototype, {
    addEventListener: {
      configurable: true,
      writable: true,
      value(type, listener, options) {
        EventTarget.prototype.addEventListener.call(this, type, listener, options);
        if (type === "message" && listener !== null) {
          this._eventMessageListeners.add(listener);
          if (options?.once) {
            this._eventMessageOnceListeners.add(listener);
          }
          this.__scheduleQueuedMessages();
        }
      },
    },
    removeEventListener: {
      configurable: true,
      writable: true,
      value(type, listener, options) {
        EventTarget.prototype.removeEventListener.call(this, type, listener, options);
        if (type === "message") {
          this._eventMessageListeners.delete(listener);
          this._eventMessageOnceListeners.delete(listener);
        }
      },
    },
    __dispatchQueuedMessages: {
      configurable: true,
      writable: true,
      value: __quenchMessagePortPrototype.__dispatchQueuedMessages,
    },
    __scheduleQueuedMessages: {
      configurable: true,
      writable: true,
      value: __quenchMessagePortPrototype.__scheduleQueuedMessages,
    },
  });
  delete __quenchMessagePortPrototype.__dispatchQueuedMessages;
  delete __quenchMessagePortPrototype.__scheduleQueuedMessages;
  Object.setPrototypeOf(__quenchMessagePortPrototype, __quenchMessagePortEventEmitterPrototype);
  Object.defineProperties(__quenchMessagePortPrototype, {
    onmessage: {
      configurable: true,
      get() { return this._onmessage; },
      set(value) { this._onmessage = value; },
    },
    onmessageerror: {
      configurable: true,
      get() { return this._onmessageerror; },
      set(value) { this._onmessageerror = value; },
    },
  });
  const __quenchMessagePortCallable = function MessagePort() {};
  __quenchMessagePortCallable.prototype = __quenchMessagePortImplementation.prototype;
  const MessagePort = new Proxy(__quenchMessagePortCallable, {
    apply() {
      const error = new TypeError("Class constructor MessagePort cannot be invoked without 'new'");
      error.code = "ERR_CONSTRUCT_CALL_INVALID";
      throw error;
    },
    construct(_target, args, newTarget) {
      return Reflect.construct(__quenchMessagePortImplementation, args, newTarget);
    },
  });
  Object.defineProperty(__quenchMessagePortImplementation.prototype, "constructor", {
    configurable: true, writable: true, value: MessagePort,
  });
  Object.defineProperty(globalThis, "MessagePort", {
    value: MessagePort, writable: true, configurable: true,
  });
  Object.defineProperty(globalThis, "__quenchCreateMessagePort", {
    configurable: true,
    value: () => new MessagePort(__quenchMessagePortToken),
  });
}

if (globalThis.MessageChannel === undefined) {
  const __quenchMessageChannelImplementation = class {
    constructor() {
      this.port1 = new globalThis.MessagePort(__quenchMessagePortToken);
      this.port2 = new globalThis.MessagePort(__quenchMessagePortToken);
      this.port1._peer = this.port2;
      this.port2._peer = this.port1;
    }
  };
  const __quenchMessageChannelCallable = function MessageChannel() {};
  __quenchMessageChannelCallable.prototype = __quenchMessageChannelImplementation.prototype;
  const MessageChannel = new Proxy(__quenchMessageChannelCallable, {
    apply() {
      const error = new TypeError("Class constructor MessageChannel cannot be invoked without 'new'");
      error.code = "ERR_CONSTRUCT_CALL_REQUIRED";
      throw error;
    },
    construct(_target, args, newTarget) {
      return Reflect.construct(__quenchMessageChannelImplementation, args, newTarget);
    },
  });
  Object.defineProperty(__quenchMessageChannelImplementation.prototype, "constructor", {
    configurable: true, writable: true, value: MessageChannel,
  });
  Object.defineProperty(globalThis, "MessageChannel", {
    value: MessageChannel, writable: true, configurable: true,
  });
}
}
"#
);
