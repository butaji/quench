const __nodeFsReadStreamOptions = (options) => {
  const start = options.start === undefined ? 0 : Number(options.start);
  const end = options.end === undefined ? undefined : Number(options.end);
  if (!Number.isInteger(start) || start < 0) {
    throw Object.assign(new RangeError('The "start" option is out of range'), { code: "ERR_OUT_OF_RANGE" });
  }
  if (end !== undefined && start > end) {
    const error = new RangeError(
      `The value of "start" is out of range. It must be <= "end" (here: ${end}). Received ${start}`,
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (end !== undefined && (!Number.isInteger(end) || end < 0)) {
    throw Object.assign(new RangeError('The "end" option is out of range'), { code: "ERR_OUT_OF_RANGE" });
  }
  return { start, end };
};
const __nodeFsReadStreamClose = (stream) => {
  if (stream._closeEmitted) return;
  stream._closeEmitted = true;
  stream.closed = true;
  if (stream._fileHandle) {
    stream.fd = null;
    Promise.resolve(stream._fileHandle.close()).then(
      () => stream.emit("close"),
      (error) => stream.emit("error", error),
    );
    return;
  }
  if (stream.fd !== null) {
    try {
      globalThis.__nodeFs.closeSync(stream.fd);
    } catch (error) {
      if (error.code !== "EBADF") throw error;
    }
    stream.fd = null;
  }
  stream.emit("close");
};
globalThis.__nodeFs.createReadStream = function (value, options = {}) {
  const encoding = typeof options === "string"
    ? options
    : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    throw Object.assign(new TypeError(`The argument 'encoding' is invalid. Received '${encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
  }
  const { start, end } = __nodeFsReadStreamOptions(options);
  const stream = options.__target || new NodeReadable(options);
  const fileHandle = options.fd && typeof options.fd === "object"
    ? options.fd
    : null;
  const suppliedFd = fileHandle ? fileHandle.fd : options.fd;
  const hasSuppliedFd = typeof suppliedFd === "number";
  const path = hasSuppliedFd
    ? globalThis.__nodeFdPaths[suppliedFd]
    : nodePathValue(value);
  stream.path = path;
  stream.fd = hasSuppliedFd ? suppliedFd : null;
  stream._fileHandle = fileHandle;
  stream.bytesRead = 0;
  stream.close = (callback) => {
    __nodeFsReadStreamClose(stream);
    if (typeof callback === "function") queueMicrotask(() => callback());
    return stream;
  };
  stream.length = 30000;
  if (options.encoding) stream.length = 10000;
  if (options.encoding) setTimeout(() => (stream.length = 10000), 1);
  if (options.encoding !== undefined) stream.setEncoding(options.encoding);
  setTimeout(() => {
    try {
      if (options.__target && typeof stream.open === "function") {
        stream.open();
        return;
      }
      if (!hasSuppliedFd) {
        stream.fd = globalThis.__nodeFs.openSync(path, "r");
        stream.emit("open", stream.fd);
      }
      const bytes = globalThis.__nodeFs.readFileSync(stream.fd);
      stream.length = bytes.length;
      const end = options.end === undefined ? bytes.length : options.end + 1;
      const offset = options.start === undefined && hasSuppliedFd
        ? globalThis.__nodeFdPositions[stream.fd] || 0
        : start;
      const chunk = bytes.subarray(offset, Math.min(end, bytes.length));
      if (hasSuppliedFd) {
        globalThis.__nodeFdPositions[stream.fd] = offset + chunk.byteLength;
      }
      if (options.autoClose !== false) {
        stream.once("end", () => __nodeFsReadStreamClose(stream));
      }
      if (chunk.length) {
        if (options.encoding) {
          stream.emit("data", chunk.toString(options.encoding));
        } else stream.push(chunk);
        stream.bytesRead += chunk.byteLength;
      }
      stream.push(null);
    } catch (error) {
      stream.emit("error", error);
    }
  }, 0);
  return stream;
};
globalThis.__nodeFs.openAsBlob = async (value, options = {}) => {
  const data = globalThis.__nodeFs.readFileSync(value);
  return new Blob([data], { type: options.type || "" });
};
globalThis.__nodeFs.ReadStream = function ReadStream(value, options = {}) {
  Object.assign(
    this,
    new NodeReadable({ ...options, __quenchCompatConstruct: true }),
  );
  this.once("error", () => this.destroy());
  globalThis.__nodeFs.createReadStream(value, { ...options, __target: this });
};
globalThis.__nodeFs.ReadStream.prototype = Object.create(
  NodeReadable.prototype,
);
globalThis.__nodeFs.ReadStream.prototype.constructor =
  globalThis.__nodeFs.ReadStream;
class NodeAbortSignal {
  constructor(reason) {
    this.aborted = false;
    this.reason = reason;
    this.onabort = null;
    this._listeners = [];
  }
  addEventListener(event, listener, options) {
    if (event === "abort") {
      const wrapped = options?.once
        ? (...args) => {
          this.removeEventListener(event, wrapped);
          listener(...args);
        }
        : listener;
      this._listeners.push(wrapped);
    }
  }
  removeEventListener(event, listener) {
    this._listeners = this._listeners.filter((item) => item !== listener);
  }
  listenerCount(event) {
    return event === "abort" ? this._listeners.length : 0;
  }
  throwIfAborted() {
    if (this.aborted) throw this.reason;
  }
  dispatchEvent(event) {
    if (event?.type !== "abort") return true;
    this._listeners.slice().forEach((listener) => listener.call(this, event));
    if (typeof this.onabort === "function") this.onabort.call(this, event);
    return !event.defaultPrevented;
  }
  static abort(reason) {
    if (reason === undefined) {
      reason = new DOMException("This operation was aborted", "AbortError");
    }
    const signal = new NodeAbortSignal(reason);
    signal.aborted = true;
    return signal;
  }
  static any(signals) {
    if (
      !Array.isArray(signals) &&
      !(signals && typeof signals[Symbol.iterator] === "function")
    ) {
      throw Object.assign(new TypeError('The "signals" argument must be an instance of Array'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    const values = Array.from(signals);
    const combined = new NodeAbortSignal();
    const abort = (signal) => {
      if (combined.aborted) return;
      combined.aborted = true;
      combined.reason = signal.reason;
      for (const value of values) {
        if (value !== signal) clearTimeout(value.__timeoutTimer);
      }
      const event = { type: "abort", target: combined };
      combined._listeners
        .slice()
        .forEach((listener) => listener.call(combined, event));
      if (typeof combined.onabort === "function") {
        combined.onabort.call(combined, event);
      }
    };
    for (let index = 0; index < values.length; index++) {
      const signal = values[index];
      if (!(signal instanceof NodeAbortSignal)) {
        throw Object.assign(new TypeError(`signals[${index}] is not of type AbortSignal.`), { code: "ERR_INVALID_ARG_TYPE" });
      }
      if (signal.aborted) abort(signal);
      else signal.addEventListener("abort", () => abort(signal));
    }
    return combined;
  }
  static timeout(milliseconds) {
    const signal = new NodeAbortSignal();
    signal.__timeoutTimer = setTimeout(() => {
      const reason = new Error("The operation was aborted due to timeout");
      reason.name = "TimeoutError";
      signal.aborted = true;
      signal.reason = reason;
      const event = { type: "abort", target: signal };
      signal._listeners
        .slice()
        .forEach((listener) => listener.call(signal, event));
      if (typeof signal.onabort === "function") {
        signal.onabort.call(signal, event);
      }
    }, milliseconds);
    return signal;
  }
}
class NodeAbortController {
  constructor() {
    this.signal = new NodeAbortSignal();
  }
  abort(reason) {
    this.signal.aborted = true;
    this.signal.reason = reason === undefined
      ? new DOMException("This operation was aborted", "AbortError")
      : reason;
    const event = { type: "abort", target: this.signal };
    this.signal._listeners
      .slice()
      .forEach((listener) => listener.call(this.signal, event));
    if (typeof this.signal.onabort === "function") {
      this.signal.onabort.call(this.signal, event);
    }
  }
}
globalThis.AbortSignal = NodeAbortSignal;
globalThis.AbortController = NodeAbortController;
globalThis.__nodeFs.open = (value, flags, mode, callback) => {
  if (typeof flags === "function") {
    callback = flags;
    flags = "r";
    mode = undefined;
  } else if (typeof mode === "function") {
    callback = mode;
    mode = undefined;
  }
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  __nodeFsValidateMode(mode);
  const path = nodeFsPath(value);
  setTimeout(() => {
    let fd;
    try {
      fd = globalThis.__nodeFs.openSync(path, flags, mode);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, fd);
  });
};
globalThis.__nodeFs.readdir = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const encoding = typeof options === "string"
    ? options
    : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    throw Object.assign(new TypeError(`The argument 'encoding' is invalid. Received '${encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    let result;
    try {
      result = globalThis.__nodeFs.readdirSync(path, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, result);
  });
};
globalThis.__nodeFs.mkdir = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  if (
    options &&
    Object.prototype.hasOwnProperty.call(options, "recursive") &&
    typeof options.recursive !== "boolean"
  ) {
    const error = new TypeError(
      `The "options.recursive" property must be of type boolean.${
        __nodeInvalidArgSuffix(
          options.recursive,
        )
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      const created = globalThis.__nodeFs.mkdirSync(path, options);
      callback(null, created);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.watch = (value, options, listener) => {
  if (typeof options === "function") {
    listener = options;
    options = {};
  }
  const encoding = typeof options === "string"
    ? options
    : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    throw Object.assign(new TypeError(`The argument 'encoding' is invalid. Received '${encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
  }
  __nodeFsReadPath(typeof value === "number" ? false : value);
  const watcher = new globalThis.__nodeEventEmitter();
  if (typeof listener === "function") watcher.on("change", listener);
  watcher.close = () => watcher;
  return watcher;
};
const __nodeFsFileWatchers = new Map();
globalThis.__nodeFs.watchFile = (value, options, listener) => {
  if (typeof options === "function") {
    listener = options;
    options = {};
  }
  const path = __nodeFsReadPath(value);
  if (listener !== undefined && typeof listener !== "function") {
    throw Object.assign(
      new TypeError('The "listener" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  const watcher = __nodeFsFileWatchers.get(path) || {
    listeners: new Set(),
    eventListeners: new Map(),
    on(event, callback) {
      const list = this.eventListeners.get(event) || [];
      list.push(callback);
      this.eventListeners.set(event, list);
      return this;
    },
    off(event, callback) {
      this.eventListeners.set(
        event,
        (this.eventListeners.get(event) || []).filter(
          (item) => item !== callback,
        ),
      );
      return this;
    },
    emit(event, ...args) {
      for (const callback of this.eventListeners.get(event) || []) {
        callback(...args);
      }
      return this;
    },
    ref() {
      this._refed = true;
      return this;
    },
    unref() {
      this._refed = false;
      return this;
    },
    hasRef() {
      return this._refed !== false;
    },
    listenerCount(event) {
      return event === "change" ? this.listeners.size : 0;
    },
    close() {
      this.closed = true;
      return this;
    },
    _refed: true,
  };
  if (listener) watcher.listeners.add(listener);
  __nodeFsFileWatchers.set(path, watcher);
  return watcher;
};
globalThis.__nodeFs.unwatchFile = (value, listener) => {
  const path = __nodeFsReadPath(value);
  const watcher = __nodeFsFileWatchers.get(path);
  if (!watcher) return;
  if (listener) watcher.listeners.delete(listener);
  else watcher.listeners.clear();
  if (!watcher.listeners.size) {
    watcher.close();
    __nodeFsFileWatchers.delete(path);
  }
};
const __nodeFsValidateMkdtemp = (prefix, options, callback) => {
  if (
    options !== undefined &&
    typeof options !== "function" &&
    typeof options !== "string" &&
    (typeof options !== "object" || options === null)
  ) {
    throw Object.assign(
      new TypeError('The "options" argument must be a string or an object'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (
    typeof prefix !== "string" &&
    !(prefix instanceof Uint8Array) &&
    !(prefix instanceof globalThis.__nodeURL)
  ) {
    throw Object.assign(
      new TypeError(
        'The "prefix" argument must be of type string or an instance of Buffer or URL',
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  const encoding = typeof options === "string"
    ? options
    : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    throw Object.assign(new TypeError(`The argument 'encoding' is invalid. Received '${encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
  }
};
globalThis.__nodeFs.mkdtemp = (prefix, options, callback) => {
  if (typeof options === "function") callback = options;
  __nodeFsValidateMkdtemp(prefix, options, callback);
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.mkdtempSync(prefix));
    } catch (error) {
      callback(error);
    }
  });
};
