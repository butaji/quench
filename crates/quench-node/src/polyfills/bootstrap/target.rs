//! Polyfill: `target`

pub const JS: &str = quench_js_check::checked_js!(
    r#"globalThis.EventTarget ||= class EventTarget {
  constructor() {
    this._listeners = {};
  }
  addEventListener(name, listener, options = {}) {
    const passive = Boolean(options.passive);
    if (typeof listener !== "function") return undefined;
    const signal = options.signal;
    if (signal !== undefined && !(signal instanceof AbortSignal)) {
      throw new TypeError("signal must be an AbortSignal");
    }
    if (signal?.aborted) return undefined;
    const record = { listener, once: Boolean(options.once), passive, signal };
    (this._listeners[name] ||= []).push(record);
    if (signal && typeof signal.addEventListener === "function") {
      record.abort = () => this.removeEventListener(name, listener);
      signal.addEventListener("abort", record.abort, { once: true });
    }
    return undefined;
  }
  removeEventListener(name, listener) {
    this._listeners[name] = (this._listeners[name] || []).filter((record) => {
      if (record.listener !== listener) return true;
      if (record.abort) {
        record.signal?.removeEventListener("abort", record.abort);
      }
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
const __quenchEventTargetEvents = Symbol.for("quench.event_target.events");
if (EventTarget.prototype) {
  Object.defineProperty(EventTarget.prototype, __quenchEventTargetEvents, {
    configurable: true,
    get() {
      const map = new Map();
      for (const [name, records] of Object.entries(this._listeners || {})) {
        map.set(name, { size: records.length });
      }
      return map;
    },
  });
}
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
    if (type === undefined || typeof type === "symbol") {
      throw new TypeError("CustomEvent type is invalid");
    }
    if (options === null || typeof options !== "object") {
      const error = new TypeError(
        `The "options" argument must be of type object. Received type ${typeof options} (${
          String(
            options,
          )
        })`,
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    super(type, options);
    Object.defineProperty(this, "detail", {
      value: options.detail === undefined ? null : options.detail,
      enumerable: true,
    });
  }
  get [Symbol.toStringTag]() {
    return "CustomEvent";
  }
};
globalThis.NodeEventTarget ||= class NodeEventTarget {
  constructor() {
    this._listeners = {};
    this._nodeListeners = {};
  }
  addListener(name, listener, options, nodeStyle = true) {
    const callback = typeof listener === "function"
      ? function (event) {
        return listener(event);
      }
      : function (event) {
        return listener.handleEvent(event);
      };
    (this._nodeListeners[name] ||= []).push({
      listener,
      callback,
      once: Boolean(options?.once),
      nodeStyle,
    });
    const eventRecord = {
      listener: callback,
      once: Boolean(options?.once),
      passive: Boolean(options?.passive),
      signal: options?.signal,
    };
    (this._listeners[name] ||= []).push(eventRecord);
    return this;
  }
  addEventListener(name, listener, options) {
    return this.addListener(name, listener, options, false);
  }
  on(name, listener, options) {
    return this.addListener(name, listener, options);
  }
  once(name, listener, options = {}) {
    return this.addListener(name, listener, { ...options, once: true });
  }
  removeListener(name, listener) {
    const records = this._nodeListeners[name] || [];
    this._nodeListeners[name] = records.filter((record) => {
      if (record.listener !== listener) return true;
      this._listeners[name] = (this._listeners[name] || []).filter(
        (eventRecord) => eventRecord.listener !== record.callback,
      );
      return false;
    });
    return this;
  }
  removeEventListener(name, listener) {
    return this.removeListener(name, listener);
  }
  dispatchEvent(event) {
    for (const record of [...(this._listeners[event.type] || [])]) {
      if (!this._listeners[event.type]?.includes(record)) continue;
      if (record.once) this.removeEventListener(event.type, record.listener);
      record.listener.call(this, event);
      if (event._quenchImmediatePropagationStopped) break;
    }
    const result = true;
    for (const record of this._nodeListeners[event.type] || []) {
      if (record.once) this.removeListener(event.type, record.listener);
    }
    return result;
  }
  off(name, listener) {
    return this.removeListener(name, listener);
  }
  listenerCount(name) {
    return (this._nodeListeners[name] || []).length;
  }
  eventNames() {
    return Object.keys(this._nodeListeners).filter((name) =>
      this.listenerCount(name)
    );
  }
  removeAllListeners(name) {
    for (const event of name === undefined ? this.eventNames() : [name]) {
      for (const record of this._nodeListeners[event] || []) {
        this._listeners[event] = (this._listeners[event] || []).filter(
          (eventRecord) => eventRecord.listener !== record.callback,
        );
      }
      delete this._nodeListeners[event];
    }
    return this;
  }
  emit(name, ...args) {
    if (!name) {
      throw new TypeError("The event name is required [ERR_INVALID_ARG_TYPE]");
    }
    const event = args[0] instanceof Event ? args[0] : new Event(name);
    let delivered = false;
    for (const record of [...(this._nodeListeners[name] || [])]) {
      if (record.nodeStyle) record.listener(...args);
      else record.callback(event);
      delivered = true;
      if (record.once) this.removeListener(name, record.listener);
    }
    return delivered;
  }
};
globalThis.MessageEvent ||= class MessageEvent extends Event {
  constructor(type, options = {}) {
    super(type, options);
    this.data = options.data === undefined ? null : options.data;
    this.origin = options.origin === undefined ? "" : String(options.origin);
    this.lastEventId = options.lastEventId === undefined
      ? ""
      : String(options.lastEventId);
    if (
      options.source !== undefined &&
      options.source !== null &&
      !(
        options.source instanceof MessagePort ||
        typeof options.source.postMessage === "function" ||
        typeof options.source.start === "function"
      )
    ) {
      throw new TypeError(
        `MessageEvent constructor: Expected eventInitDict.source ("${
          String(
            options.source,
          )
        }") to be an instance of MessagePort.`,
      );
    }
    this.source = options.source === undefined ? null : options.source;
    if (options.ports === undefined) this.ports = [];
    else {
      if (
        options.ports === null ||
        typeof options.ports[Symbol.iterator] !== "function"
      ) {
        throw new TypeError(
          `MessageEvent constructor: eventInitDict.ports (${
            String(
              options.ports,
            )
          }) is not iterable.`,
        );
      }
      this.ports = [...options.ports];
      if (
        this.ports.some(
          (port) =>
            !(
              port instanceof MessagePort ||
              typeof port?.postMessage === "function"
            ),
        )
      ) {
        throw new TypeError(
          "MessageEvent constructor: Expected eventInitDict.ports to contain MessagePort instances.",
        );
      }
    }
  }
};
globalThis.MessagePort ||= class MessagePort extends EventTarget {
  constructor() {
    super();
    Object.defineProperties(this, {
      onmessage: { value: null, writable: true, configurable: true },
      _peer: { value: null, writable: true, configurable: true },
      _started: { value: false, writable: true, configurable: true },
      _closed: { value: false, writable: true, configurable: true },
      _refed: { value: false, writable: true, configurable: true },
      _nodeListeners: { value: new Map(), writable: true, configurable: true },
    });
  }
  start() {
    this._started = true;
  }
  close(callback) {
    this._closed = true;
    this.dispatchEvent(new Event("close"));
    if (this._peer && !this._peer._closed) {
      this._peer._closed = true;
      this._peer.dispatchEvent(new Event("close"));
    }
    if (typeof callback === "function") queueMicrotask(() => callback());
  }
  ref() {
    this._refed = true;
  }
  unref() {
    this._refed = false;
  }
  hasRef() {
    return this._refed;
  }
  [Symbol.for("nodejs.util.inspect.custom")]() {
    return `MessagePort { active: ${!this._closed}, refed: ${this._refed} }`;
  }
  on(name, listener) {
    if (name === "message") this._refed = true;
    const listeners = this._nodeListeners.get(name) || [];
    listeners.push(listener);
    this._nodeListeners.set(name, listeners);
    return this;
  }
  addListener(name, listener) {
    return this.on(name, listener);
  }
  removeListener(name, listener) {
    const listeners = this._nodeListeners.get(name) || [];
    const index = listeners.indexOf(listener);
    if (index >= 0) listeners.splice(index, 1);
    return this;
  }
  emit(name, ...args) {
    for (const listener of this._nodeListeners.get(name) || []) {
      listener(...args);
    }
    if (name === "message") {
      const event = new MessageEvent("message", { data: args[0] });
      const delivered = this.dispatchEvent(event);
      if (typeof this.onmessage === "function") {
        this.onmessage.call(this, event);
        return true;
      }
      return delivered;
    }
    return this.dispatchEvent(new CustomEvent(name, { detail: args[0] }));
  }
  postMessage(value, transferList) {
    const hasTransferOption = transferList !== null &&
      typeof transferList === "object" && !Array.isArray(transferList) &&
      typeof transferList[Symbol.iterator] !== "function";
    const transfers = hasTransferOption
      ? transferList.transfer
      : transferList === null ? undefined : transferList;
    const transferArgument = hasTransferOption ? "options.transfer" : "transferList";
    if (transfers !== undefined &&
        (transfers === null || typeof transfers[Symbol.iterator] !== "function")) {
      throw Object.assign(
        new TypeError(`Optional ${transferArgument} argument must be an iterable`),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    const transferValues = transfers === undefined ? [] : Array.from(transfers);
    if (transferValues.includes(this)) {
      throw new DOMException("Transfer list contains source port", "DataCloneError");
    }
    if (transferValues.some((item) =>
      item instanceof MessagePort && item._closed)) {
      throw new DOMException(
        "MessagePort in transfer list is already detached",
        "DataCloneError",
      );
    }
    const seenTransfers = new Set();
    for (const item of transferValues) {
      if (seenTransfers.has(item)) {
        const type = item instanceof ArrayBuffer
          ? "ArrayBuffer"
          : item instanceof MessagePort ? "MessagePort" : "object";
        throw new DOMException(`Transfer list contains duplicate ${type}`, "DataCloneError");
      }
      seenTransfers.add(item);
    }
    if (transferValues.some((item) => __nodeUntransferableBuffers.has(item))) {
      const error = new DOMException(
        "ArrayBuffer is not transferable",
        "DataCloneError",
      );
      error.code = 25;
      throw error;
    }
    if (this._closed || !this._peer || this._peer._closed) return;
    // MessagePort uses the same structured clone options overload as
    // structuredClone: the second argument may be either an iterable or an
    // options object containing `transfer`.
    const cloneTransfers = transferValues.filter((item) => !(item instanceof MessagePort));
    const data = typeof structuredClone === "function"
      ? structuredClone(value, transfers === undefined ? undefined : { transfer: cloneTransfers })
      : value;
    const event = new CustomEvent("message", { detail: data });
    Object.defineProperty(event, "data", { value: data, enumerable: true });
    queueMicrotask(() => {
      if (this._peer._closed) return;
      this._peer.dispatchEvent(event);
      for (const listener of this._peer._nodeListeners.get("message") || []) {
        listener(data);
      }
      if (typeof this._peer.onmessage === "function") {
        this._peer.onmessage.call(this._peer, event);
      }
    });
  }
};
globalThis.MessageChannel ||= class MessageChannel {
  constructor() {
    this.port1 = new MessagePort();
    this.port2 = new MessagePort();
    this.port1._peer = this.port2;
    this.port2._peer = this.port1;
  }
};
for (const name of ["MessageChannel", "MessagePort"]) {
  if (name in globalThis) {
    Object.defineProperty(globalThis, name, {
      configurable: true,
      enumerable: false,
      writable: true,
      value: globalThis[name],
    });
  }
}
const __quenchEventTargetAdd = EventTarget.prototype.addEventListener;
const __quenchEventTargetRemove = EventTarget.prototype.removeEventListener;
const __quenchEventTargetDispatch = EventTarget.prototype.dispatchEvent;
const __quenchPassiveListeners = new WeakMap();
EventTarget.prototype.addEventListener = function (name, listener, options) {
  const passive = Boolean(options?.passive);
  if (!passive || typeof listener !== "function") return __quenchEventTargetAdd.call(this, name, listener, options);
  const wrapper = (event) => { event._quenchPassive = true; listener.call(this, event); event._quenchPassive = false; };
  let listeners = __quenchPassiveListeners.get(this);
  if (!listeners) __quenchPassiveListeners.set(this, listeners = new Map());
  listeners.set(listener, wrapper);
  return __quenchEventTargetAdd.call(this, name, wrapper, options);
};
EventTarget.prototype.removeEventListener = function (name, listener, options) {
  const wrapper = __quenchPassiveListeners.get(this)?.get(listener) || listener;
  __quenchPassiveListeners.get(this)?.delete(listener);
  return __quenchEventTargetRemove.call(this, name, wrapper, options);
};
EventTarget.prototype.dispatchEvent = function (event) {
  event.target = this; event.currentTarget = this;
  try { Object.defineProperty(event, "eventPhase", { value: 2, configurable: true }); } catch (_) {}
  event._quenchPath = [this];
  const result = __quenchEventTargetDispatch.call(this, event);
  return result && !event.defaultPrevented;
};
if (globalThis.Event && !Event.prototype.stopImmediatePropagation) {
  Event.prototype.stopImmediatePropagation = function () { this._quenchImmediatePropagationStopped = true; };
}
if (globalThis.Event && !Event.prototype.__quenchPassivePreventDefault) {
  const originalPreventDefault = Event.prototype.preventDefault;
  Event.prototype.preventDefault = function () { if (!this._quenchPassive) originalPreventDefault.call(this); };
  Event.prototype.__quenchPassivePreventDefault = true;
}
if (globalThis.Event) {
  Event.prototype.timeStamp ||= Date.now();
  Event.prototype.composedPath ||= function () { return this._quenchPath || []; };
  Event.prototype.returnValue ??= true;
  Event.prototype.isTrusted ??= false;
  Event.prototype.eventPhase ??= 0;
  Event.prototype.cancelBubble ??= false;
  try { Object.defineProperty(Event.prototype, "cancelBubble", { get() { return Boolean(this._quenchCancelBubble); }, set(value) { this._quenchCancelBubble = Boolean(value); }, configurable: true }); } catch (_) {}
  Event.prototype.stopPropagation ||= function () { this.cancelBubble = true; };
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
    if (typeof listener !== "function") {
      return originalAddEventListener.call(this, type, listener, options);
    }
    return originalAddEventListener.call(
      this,
      type,
      (event) =>
        listener.call(this, event || { stopImmediatePropagation() {} }),
      options,
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
    throw Object.assign(new TypeError("The setMaxListeners argument must be a number [ERR_INVALID_ARG_TYPE]"), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (Number.isNaN(limit) || limit < 0) {
    throw Object.assign(new RangeError("The value of setMaxListeners is out of range [ERR_OUT_OF_RANGE]"), { code: "ERR_OUT_OF_RANGE" });
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
      throw Object.assign(new TypeError("The eventTarget argument must be an instance of EventEmitter or EventTarget [ERR_INVALID_ARG_TYPE]"), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (target instanceof AbortSignal) return 0;
    return limits.get(target) ?? 10;
  };
  __quenchEventsModule.setMaxListeners = (limit, ...targets) => {
    __quenchValidateEventLimit(limit);
    if (targets.length === 0 &&
        typeof globalThis.__quench_events_set_max === "function") {
      globalThis.__quench_events_set_max(limit);
    }
    for (const target of targets) {
      if (!__quenchEventsTargetValid(target)) {
        throw Object.assign(new TypeError("The eventTargets argument must be an instance of EventEmitter or EventTarget [ERR_INVALID_ARG_TYPE]"), { code: "ERR_INVALID_ARG_TYPE" });
      }
      limits.set(target, limit);
      if (target instanceof EventTarget &&
          typeof globalThis.__quench_events_set_max === "function") {
        globalThis.__quench_events_set_max(limit, target);
      }
    }
    return targets[0];
  };
  const originalListenerCount = __quenchEventsModule.listenerCount;
  __quenchEventsModule.listenerCount = (target, event, listener) => {
    if (target instanceof AbortSignal) {
      return event === "abort" ? target._listeners?.length || 0 : 0;
    }
    return originalListenerCount.call(
      __quenchEventsModule,
      target,
      event,
      listener,
    );
  };
  return __quenchEventsModule;
};
globalThis.require = (name) => {
  const value = __quenchEventsOriginalRequire(name);
  if (String(name).replace(/^node:/, "") !== "events") return value;
  return __quenchEventsRequire(value);
};
"#
);
