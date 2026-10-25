//! Same-runtime BroadcastChannel implementation for the embedded worker API.

pub const JS: &str = quench_js_check::checked_js!(r#"
const __quenchBroadcastChannels = globalThis.__quenchBroadcastChannels ||
  Object.defineProperty(globalThis, "__quenchBroadcastChannels", {
    value: new Map(), configurable: true,
  }).__quenchBroadcastChannels;
let __quenchBroadcastFlushScheduled = false;
const __quenchEnqueueBroadcastMessage = (name, data, excluded) => {
  const group = __quenchBroadcastChannels.get(name);
  if (!group) return;
  for (const target of [...group]) {
    if (target === excluded || target._closed) continue;
    target._queue.push({ data, canceled: false });
  }
  if (!__quenchBroadcastFlushScheduled) {
    __quenchBroadcastFlushScheduled = true;
    queueMicrotask(__quenchFlushBroadcastMessages);
  }
};
const __quenchFlushBroadcastMessages = () => {
  __quenchBroadcastFlushScheduled = false;
  for (const group of __quenchBroadcastChannels.values()) {
    for (const target of group) {
      while (!target._closed && target._queue.length) {
        const message = target._queue.shift();
        if (message.canceled) continue;
        const event = new MessageEvent("message", { data: message.data });
        Object.defineProperty(event, "target", { configurable: true, value: target });
        Object.defineProperty(event, "currentTarget", { configurable: true, value: target });
        target.dispatchEvent(event);
      }
    }
  }
};
Object.defineProperty(globalThis, "__quenchBroadcastReceive", {
  configurable: true,
  value: (name, data) => __quenchEnqueueBroadcastMessage(name, data),
});
const __quenchReceiveMessageOnPort = function receiveMessageOnPort(port) {
  if (!(port instanceof MessagePort) && !(port instanceof __quenchBroadcastChannelClass)) {
    throw Object.assign(
      new TypeError('The "port" argument must be a MessagePort instance'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (port._closed) return undefined;
  const message = port._queue.shift();
  if (!message) return undefined;
  message.canceled = true;
  return { message: message.data };
};
const __quenchBroadcastChannelClass = class BroadcastChannel extends EventTarget {
  static __invalidThis(name) {
    const error = new TypeError(`BroadcastChannel.prototype.${name} called on an incompatible receiver`);
    error.code = "ERR_INVALID_THIS";
    throw error;
  }
  constructor(name) {
    super();
    if (arguments.length === 0) {
      const error = new TypeError('The "name" argument must be specified');
      error.code = "ERR_MISSING_ARGS";
      throw error;
    }
    if (typeof name === "symbol") throw new TypeError("Cannot convert a Symbol value to a string");
    this._name = String(name);
    this._closed = false;
    this._refed = true;
    this._queue = [];
    this._onmessage = null;
    this._onmessageerror = null;
    let group = __quenchBroadcastChannels.get(this._name);
    if (!group) __quenchBroadcastChannels.set(this._name, group = new Set());
    group.add(this);
    globalThis.__quenchBroadcastRef?.(true);
  }
  get name() { if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("name"); return this._name; }
  get onmessage() { if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("onmessage"); return this._onmessage; }
  set onmessage(value) {
    if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("onmessage");
    this._onmessage = value == null ? null : value;
    if (value == null) {
      if (this._onmessageListener) this.removeEventListener("message", this._onmessageListener);
    } else {
      if (!this._onmessageListener) this._onmessageListener = (event) => {
        if (typeof this._onmessage === "function") this._onmessage.call(this, event);
      };
      this.addEventListener("message", this._onmessageListener);
    }
  }
  get onmessageerror() { if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("onmessageerror"); return this._onmessageerror; }
  set onmessageerror(value) { if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("onmessageerror"); this._onmessageerror = value == null ? null : value; }
  close() {
    if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("close");
    if (this._closed) return;
    this._closed = true;
    if (this._refed) {
      this._refed = false;
      globalThis.__quenchBroadcastRef?.(false);
    }
    const group = __quenchBroadcastChannels.get(this._name);
    group?.delete(this);
    if (group?.size === 0) __quenchBroadcastChannels.delete(this._name);
  }
  postMessage(message) {
    if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("postMessage");
    if (this._closed) throw new Error("BroadcastChannel is closed");
    if (arguments.length === 0) {
      const error = new TypeError('The "message" argument must be specified');
      error.code = "ERR_MISSING_ARGS";
      throw error;
    }
    if (typeof message === "symbol") throw new DOMException("Symbol() could not be cloned", "DataCloneError");
    if (message && typeof message === "object" &&
        (message instanceof MessagePort || message instanceof MessageChannel)) {
      throw new DOMException("Object that needs transfer was found in message but not listed in transferList", "DataCloneError");
    }
    __quenchEnqueueBroadcastMessage(this._name, message, this);
    // Worker runtimes have separate globals. The hidden native bridge fans
    // the cloned message out to matching channels in other worker threads.
    globalThis.__quenchBroadcastPost?.(this._name, message);
  }
  ref() {
    if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("ref");
    if (!this._closed && !this._refed) {
      this._refed = true;
      globalThis.__quenchBroadcastRef?.(true);
    }
    return this;
  }
  unref() {
    if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("unref");
    if (!this._closed && this._refed) {
      this._refed = false;
      globalThis.__quenchBroadcastRef?.(false);
    }
    return this;
  }
  hasRef() {
    if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("hasRef");
    return !this._closed && this._refed;
  }
  [Symbol.for("nodejs.util.inspect.custom")](depth) {
    if (!(this instanceof BroadcastChannel)) BroadcastChannel.__invalidThis("[nodejs.util.inspect.custom]");
    if (depth < 0) return "BroadcastChannel";
    const name = this._name.replaceAll("\\", "\\\\").replaceAll("'", "\\'");
    return `BroadcastChannel { name: '${name}', active: ${!this._closed} }`;
  }
};
// Node gives both the worker_threads implementation and the global constructor
// the standard brand used by Object.prototype.toString. The worker_threads
// export below is always this class, even if the host already supplied a
// global BroadcastChannel constructor.
Object.defineProperty(__quenchBroadcastChannelClass.prototype, Symbol.toStringTag, {
  configurable: true,
  value: "BroadcastChannel",
});
// Node exposes BroadcastChannel globally as well as from `worker_threads`.
// Keep the same constructor identity so instances participate in the native
// worker-thread bridge installed below.
if (typeof globalThis.BroadcastChannel !== "function") {
  Object.defineProperty(globalThis, "BroadcastChannel", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: __quenchBroadcastChannelClass,
  });
}
// Some hosts provide BroadcastChannel before this polyfill runs. Ensure that
// constructor has the same prototype brand as the worker_threads export.
if (typeof globalThis.BroadcastChannel === "function") {
  Object.defineProperty(globalThis.BroadcastChannel.prototype, Symbol.toStringTag, {
    configurable: true,
    value: "BroadcastChannel",
  });
}
Object.defineProperty(globalThis, "__quenchBroadcastChannelClass", {
  configurable: true,
  value: __quenchBroadcastChannelClass,
});
Object.defineProperty(globalThis, "__quenchReceiveMessageOnPort", {
  configurable: true,
  value: __quenchReceiveMessageOnPort,
});
"#);
