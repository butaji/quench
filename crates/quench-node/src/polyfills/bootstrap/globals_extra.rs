//! Polyfill: `globals-extra`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeDomExceptionCodes = {
  IndexSizeError: 1,
  HierarchyRequestError: 3,
  WrongDocumentError: 4,
  InvalidCharacterError: 5,
  NoModificationAllowedError: 7,
  NotFoundError: 8,
  NotSupportedError: 9,
  InUseAttributeError: 10,
  InvalidStateError: 11,
  SyntaxError: 12,
  InvalidModificationError: 13,
  NamespaceError: 14,
  TypeMismatchError: 17,
  SecurityError: 18,
  NetworkError: 19,
  AbortError: 20,
  URLMismatchError: 21,
  QuotaExceededError: 22,
  TimeoutError: 23,
  InvalidNodeTypeError: 24,
  DataCloneError: 25,
};
if (typeof globalThis.require === "function" && !globalThis.worker_threads) {
  globalThis.worker_threads = globalThis.require("worker_threads");
}
for (const name of ["worker_threads", "__nodeCurrentAsyncResource", "__nodeCallChecks"]) {
  if (name in globalThis) {
    Object.defineProperty(globalThis, name, {
      configurable: true,
      enumerable: false,
      writable: true,
      value: globalThis[name],
    });
  }
}
globalThis.DOMException = class DOMException extends Error {
  constructor(message = "", name = "Error") {
    super(message);
    this.name = name;
    this.code = __nodeDomExceptionCodes[name] || 0;
    this["\0domexception"] = true;
  }
  toString() {
    return `${this.name}: ${this.message}`;
  }
};
if (typeof globalThis.TypeMismatchError !== "function") {
  globalThis.TypeMismatchError = class TypeMismatchError extends DOMException {
    constructor(message = "") { super(message, "TypeMismatchError"); }
  };
}
if (typeof globalThis.QuotaExceededError !== "function") {
  globalThis.QuotaExceededError = class QuotaExceededError extends DOMException {
    constructor(message = "", options = null) {
      super(message, "QuotaExceededError");
      let quota = null;
      let requested = null;
      if (options !== null && options !== undefined) {
        if (typeof options !== "object" && typeof options !== "function") {
          throw new TypeError("The options argument must be an object");
        }
        const read = (name) => {
          if (!(name in options) || options[name] === undefined) return null;
          const value = Number(options[name]);
          if (!Number.isFinite(value)) throw new TypeError(`The ${name} option must be a finite number`);
          if (value < 0) throw new RangeError(`The ${name} option must be non-negative`);
          return value;
        };
        quota = read("quota");
        requested = read("requested");
        if (quota !== null && requested !== null && requested < quota) {
          throw new RangeError("requested must be greater than or equal to quota");
        }
      }
      this._quota = quota;
      this._requested = requested;
    }
    get quota() {
      if (!(this instanceof QuotaExceededError)) throw Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
      return this._quota;
    }
    get requested() {
      if (!(this instanceof QuotaExceededError)) throw Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
      return this._requested;
    }
  };
  Object.defineProperty(globalThis.QuotaExceededError.prototype, "quota", { enumerable: true, configurable: true });
  Object.defineProperty(globalThis.QuotaExceededError.prototype, "requested", { enumerable: true, configurable: true });
  Object.defineProperty(globalThis.QuotaExceededError.prototype, Symbol.toStringTag, {
    configurable: true,
    value: "QuotaExceededError",
  });
}
for (
  const [name, value] of Object.entries({
    INDEX_SIZE_ERR: 1,
    DOMSTRING_SIZE_ERR: 2,
    HIERARCHY_REQUEST_ERR: 3,
    WRONG_DOCUMENT_ERR: 4,
    INVALID_CHARACTER_ERR: 5,
    NO_MODIFICATION_ALLOWED_ERR: 7,
    NOT_FOUND_ERR: 8,
    NOT_SUPPORTED_ERR: 9,
    INUSE_ATTRIBUTE_ERR: 10,
    INVALID_STATE_ERR: 11,
    SYNTAX_ERR: 12,
    INVALID_MODIFICATION_ERR: 13,
    NAMESPACE_ERR: 14,
    TYPE_MISMATCH_ERR: 17,
    SECURITY_ERR: 18,
    NETWORK_ERR: 19,
    ABORT_ERR: 20,
    URL_MISMATCH_ERR: 21,
    QUOTA_EXCEEDED_ERR: 22,
    TIMEOUT_ERR: 23,
    INVALID_NODE_TYPE_ERR: 24,
    DATA_CLONE_ERR: 25,
  })
) {
  Object.defineProperty(globalThis.DOMException, name, {
    configurable: false,
    enumerable: true,
    value,
    writable: false,
  });
}
if (!globalThis.navigator) {
  const platform = globalThis.__quench_platform === "macos"
    ? "MacIntel"
    : globalThis.__quench_platform === "linux"
    ? "Linux x86_64"
    : globalThis.__quench_platform === "windows"
    ? "Win32"
    : String(globalThis.__quench_platform || "");
  const navigator = {};
  for (
    const [name, value] of [
      ["userAgent", "Node.js/20"],
      ["language", "en-US"],
      ["languages", ["en-US"]],
      ["hardwareConcurrency", 1],
      ["platform", platform],
    ]
  ) {
    Object.defineProperty(navigator, name, {
      configurable: true,
      enumerable: true,
      value: Array.isArray(value) ? Object.freeze(value) : value,
      writable: false,
    });
  }
  globalThis.navigator = Object.freeze(navigator);
}
if (typeof globalThis.Blob !== "function" ||
    typeof globalThis.Blob.prototype?.arrayBuffer !== "function") {
  if (typeof globalThis.ReadableStream !== "function") {
    class __nodeBlobReadableStream {
      constructor(source = {}) {
        this._queue = [];
        this._closed = false;
        this._waiters = [];
        const controller = {
          enqueue: (value) => {
            const waiter = this._waiters.shift();
            if (waiter) waiter({ value, done: false });
            else this._queue.push(value);
          },
          close: () => {
            this._closed = true;
            while (this._waiters.length) this._waiters.shift()({ value: undefined, done: true });
          },
        };
        source.start?.(controller);
      }
      getReader() {
        const stream = this;
        return { read() {
          if (stream._queue.length) return Promise.resolve({ value: stream._queue.shift(), done: false });
          if (stream._closed) return Promise.resolve({ value: undefined, done: true });
          return new Promise((resolve) => stream._waiters.push(resolve));
        }, releaseLock() {} };
      }
      async *[Symbol.asyncIterator]() {
        const reader = this.getReader();
        for (;;) { const item = await reader.read(); if (item.done) return; yield item.value; }
      }
    }
    globalThis.ReadableStream = __nodeBlobReadableStream;
  }
  const __nodeBlobPart = (part) => {
    if (typeof part === "string") return Buffer.from(part);
    if (part instanceof ArrayBuffer) return Buffer.from(part);
    // Blob consumes the viewed bytes, not typed-array element values.  Using
    // the view's byte range also preserves the correct width for Uint16 /
    // Uint32 and floating-point views.
    if (ArrayBuffer.isView(part)) {
      return Buffer.from(new Uint8Array(part.buffer, part.byteOffset, part.byteLength));
    }
    if (part && part._data && typeof part._data.byteLength === "number") {
      return Buffer.from(part._data);
    }
    return Buffer.from(String(part));
  };
  const __nodeBlobEnsureReadable = (blob) => {
    const path = blob?.["\0quench:file-backed:path"];
    if (typeof path !== "string") return;
    let stat;
    try {
      stat = globalThis.__nodeFs?.statSync(path);
    } catch (_) {
      stat = undefined;
    }
    const size = blob["\0quench:file-backed:size"];
    const mtime = blob["\0quench:file-backed:mtime"];
    if (!stat || Number(stat.size) !== Number(size) || Math.trunc(Number(stat.mtimeMs)) !== Number(mtime)) {
      throw new DOMException("The file has been modified", "NotReadableError");
    }
  };
  Object.defineProperty(globalThis, "__quenchBlobEnsureReadable", {
    configurable: true,
    value: __nodeBlobEnsureReadable,
  });
  class Blob {
    constructor(parts = [], options = {}) {
      if (!Array.isArray(parts)) {
        throw Object.assign(
          new TypeError('The "sources" argument must be an instance of Array'),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
      if (options !== undefined && options !== null &&
          typeof options !== "object" && typeof options !== "function") {
        throw Object.assign(
          new TypeError('The "options" argument must be of type object.'),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      const endings = options?.endings;
      if (endings !== undefined && endings !== "transparent" && endings !== "native") {
        throw Object.assign(
          new TypeError(`The property 'options.endings' must be one of: 'transparent', 'native'. Received ${String(endings)}`),
          { code: "ERR_INVALID_ARG_VALUE" },
        );
      }
      const nativeEol = process.platform === "win32" ? "\r\n" : "\n";
      const normalizedParts = endings === "native"
        ? parts.map((part) => typeof part === "string"
          ? part.replace(/\r\n|\r|\n/g, nativeEol)
          : part)
        : parts;
      this._parts = normalizedParts.map(__nodeBlobPart);
      this._data = Buffer.concat(this._parts);
      this._size = this._data.byteLength;
      // Blob's type option stringifies every supplied value, including
      // falsy values such as `false` and `0`; only an omitted/nullish option
      // defaults to the empty MIME type.
      const type = String(options?.type ?? "").toLowerCase();
      this._type = /^[\x20-\x7e]*$/.test(type) ? type : "";
    }
    async arrayBuffer() {
      if (!this || !this._data) throw Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
      __nodeBlobEnsureReadable(this);
      return this._data.buffer.slice(
        this._data.byteOffset,
        this._data.byteOffset + this._data.byteLength
      );
    }
    async text() {
      if (!this || !this._data) throw Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
      __nodeBlobEnsureReadable(this);
      return this._data.toString();
    }
    slice(start = 0, end = this.size, type = "") {
      if (!this || !this._data) throw Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
      __nodeBlobEnsureReadable(this);
      const normalize = (value, fallback, label) => {
        if (typeof value === "bigint") {
          throw Object.assign(new TypeError(`${label} is a BigInt and cannot be converted to a number.`), { code: "ERR_INVALID_ARG_TYPE" });
        }
        if (typeof value === "symbol") {
          throw Object.assign(new TypeError(`${label} is a Symbol and cannot be converted to a number.`), { code: "ERR_INVALID_ARG_TYPE" });
        }
        const number = Number(value);
        if (!Number.isFinite(number)) return fallback;
        return number < 0 ? Math.max(this.size + number, 0) : Math.min(number, this.size);
      };
      return new Blob([
        this._data.subarray(normalize(start, 0, "start"), normalize(end, this.size, "end"))
      ], { type });
    }
    stream() {
      if (!this || !this._data) throw Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
      const chunks = this._parts || [this._data];
      const Stream = globalThis.ReadableStream || globalThis.__quenchReadableStream;
      const thisBlob = this;
      let index = 0;
      return new Stream({
        pull(controller) {
          __nodeBlobEnsureReadable(thisBlob);
          if (index < chunks.length) controller.enqueue(chunks[index++]);
          else controller.close();
        }
      }, { highWaterMark: 0, size: (chunk) => chunk.byteLength });
    }
    textStream() {
      if (!this || !this._data) throw Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
      __nodeBlobEnsureReadable(this);
      const text = this._data.toString();
      const Stream = globalThis.ReadableStream || globalThis.__quenchReadableStream;
      const stream = new Stream({
        start(controller) {
          controller.enqueue(text);
          controller.close();
        },
      });
      // Some embedded stream shims expose an async-generator method that
      // loses the stream receiver. Bind a minimal iterator to this instance
      // so Blob.textStream remains consumable with `for await`.
      const reader = stream.getReader();
      stream[Symbol.asyncIterator] = () => ({ next: () => reader.read() });
      return stream;
    }
    async bytes() {
      if (!this || !this._data) throw Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
      __nodeBlobEnsureReadable(this);
      return new Uint8Array(this._data);
    }
  }
  for (const [name, getter] of [["size", function () {
    if (!(this instanceof Blob)) throw Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
    return this._size;
  }], ["type", function () {
    if (!(this instanceof Blob)) throw Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
    return this._type;
  }]]) {
    Object.defineProperty(Blob.prototype, name, {
      get: getter, enumerable: true, configurable: true,
    });
  }
  for (const name of ["arrayBuffer", "text", "slice", "stream", "textStream", "bytes"]) {
    Object.defineProperty(Blob.prototype, name, {
      value: Blob.prototype[name], enumerable: true, configurable: true, writable: true
    });
  }
  Object.defineProperty(Blob.prototype, Symbol.toStringTag, {
    value: "Blob", configurable: true
  });
  Object.defineProperty(globalThis, "Blob", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: Blob
  });
}
if (typeof globalThis.File !== "function" && typeof globalThis.Blob === "function") {
  class File extends globalThis.Blob {
    constructor(parts = [], name = "", options = {}) {
      super(parts, options);
      this.name = String(name);
      const modified = options && options.lastModified;
      this.lastModified = modified === undefined ? Date.now() : Number(modified);
    }
  }
  Object.defineProperty(globalThis, "File", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: File
  });
}
// Native Blob methods reject an invalid receiver with an empty reason in some
// embedded builds. Normalize that edge at the host boundary to Node's
// observable ERR_INVALID_THIS contract while retaining the native semantics
// for genuine Blob instances.
if (typeof globalThis.Blob === "function" && globalThis.Blob.prototype) {
  const __quenchBlobInvalidThis = () =>
    Object.assign(new TypeError("Illegal invocation"), { code: "ERR_INVALID_THIS" });
  for (const [name, asynchronous] of [
    ["arrayBuffer", true], ["text", true], ["bytes", true],
    ["slice", false], ["stream", false], ["textStream", false],
  ]) {
    const original = globalThis.Blob.prototype[name];
    if (typeof original !== "function") continue;
    Object.defineProperty(globalThis.Blob.prototype, name, {
      configurable: true,
      enumerable: true,
      writable: true,
      value: function (...args) {
        if (!(this instanceof globalThis.Blob)) {
          const error = __quenchBlobInvalidThis();
          return asynchronous ? Promise.reject(error) : (() => { throw error; })();
        }
        // The embedded Blob fallback stores bytes in `_data`; resolve these
        // methods explicitly because async class methods can lose their
        // receiver across the VM's Promise continuation boundary.
        if (this._data && (name === "text" || name === "bytes" || name === "arrayBuffer")) {
          try {
            globalThis.__quenchBlobEnsureReadable?.(this);
          } catch (error) {
            return Promise.reject(error);
          }
          if (name === "text") return Promise.resolve(this._data.toString());
          if (name === "bytes") return Promise.resolve(new Uint8Array(this._data));
          const data = this._data;
          return Promise.resolve(data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength));
        }
        return original.apply(this, args);
      },
    });
  }
}
if (typeof globalThis.Headers !== "function") {
  const HeadersClass = class Headers {
    constructor(init) {
      this._entries = [];
      if (init instanceof globalThis.Headers) {
        for (const [key, value] of init.entries()) this.append(key, value);
      } else if (init instanceof Map) {
        for (const [key, value] of init.entries()) this.set(key, value);
      } else if (Array.isArray(init)) {
        for (const pair of init) if (Array.isArray(pair) && pair.length >= 2) this.append(pair[0], pair[1]);
      } else if (init && typeof init === "object") {
        for (const key of Object.keys(init)) this.set(key, init[key]);
      }
    }
    _key(key) { return String(key).toLowerCase(); }
    append(key, value) { this._entries.push([this._key(key), String(value)]); }
    set(key, value) {
      const normalized = this._key(key);
      this._entries = this._entries.filter(([name]) => name !== normalized);
      if (Array.isArray(value)) {
        for (const item of value) this.append(normalized, item);
      } else {
        this.append(normalized, value);
      }
    }
    get(key) {
      const normalized = this._key(key);
      const values = this._entries.filter(([name]) => name === normalized).map(([, value]) => value);
      return values.length ? values.join(", ") : null;
    }
    *entries() { yield* this._entries; }
    keys() { return this._entries.map(([name]) => name)[Symbol.iterator](); }
    values() { return this._entries.map(([, value]) => value)[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
  };
  Object.defineProperty(globalThis, 'Headers', {
    value: HeadersClass,
    writable: true,
    configurable: true
  });
}
const Headers = globalThis.Headers;
if (typeof globalThis.Request !== "function") {
  class Request {
    constructor(input, init = {}) {
      const source = input instanceof Request ? input : null;
      this.url = String(source?.url || input || "");
      this.method = String(init.method ?? source?.method ?? "GET").toUpperCase();
      this.headers = new Headers(init.headers ?? source?.headers);
      this._body = init.body ?? source?._body ?? null;
      this.bodyUsed = false;
      this.signal = init.signal ?? source?.signal ?? new AbortController().signal;
    }
    async text() {
      this.bodyUsed = true;
      return this._body == null ? "" : String(this._body);
    }
    async json() { return JSON.parse(await this.text()); }
    clone() {
      if (this.bodyUsed) throw new TypeError("Body has already been consumed.");
      return new Request(this);
    }
  }
  Object.defineProperty(globalThis, "Request", {
    configurable: true, enumerable: false, writable: true, value: Request,
  });
}
if (typeof globalThis.Response !== "function") {
  class Response {
    constructor(body = null, init = {}) {
      this.status = Number(init.status ?? 200);
      this.statusText = String(init.statusText ?? "");
      this.headers = new Headers(init.headers);
      this._body = body instanceof globalThis.Blob ? body : body == null ? "" : String(body);
      this.bodyUsed = false;
      this.ok = this.status >= 200 && this.status < 300;
    }
    async text() {
      this.bodyUsed = true;
      return this._body instanceof globalThis.Blob ? this._body.text() : this._body;
    }
    async bytes() {
      this.bodyUsed = true;
      return this._body instanceof globalThis.Blob
        ? this._body.bytes()
        : new TextEncoder().encode(this._body);
    }
    async arrayBuffer() { return (await this.bytes()).buffer; }
    async json() { return JSON.parse(await this.text()); }
    async blob() {
      if (this._body instanceof globalThis.Blob) return this._body;
      return new globalThis.Blob([await this.bytes()], {
        type: this.headers.get("content-type") || "",
      });
    }
    clone() {
      return new Response(this._body, {
        status: this.status,
        statusText: this.statusText,
        headers: this.headers,
      });
    }
  }
  Object.defineProperty(globalThis, "Response", {
    configurable: true, enumerable: false, writable: true, value: Response,
  });
}
if (typeof globalThis.fetch === "function" && !globalThis.fetch.__quenchBlobFetch) {
  const __quenchHostFetch = globalThis.fetch;
  const __quenchBlobFetch = function (input, init = {}) {
    const url = String(input && typeof input === "object" && "url" in input ? input.url : input);
    if (!url.startsWith("blob:")) return __quenchHostFetch(input, init);
    const buffer = globalThis.require?.("buffer");
    const blob = buffer?.resolveObjectURL?.(url);
    if (blob === undefined) return Promise.reject(new TypeError("Invalid blob URL"));
    return Promise.resolve(new globalThis.Response(blob, {
      status: 200,
      headers: blob.type ? { "content-type": blob.type } : {},
    }));
  };
  Object.defineProperty(__quenchBlobFetch, "__quenchBlobFetch", { value: true });
  Object.defineProperty(globalThis, "fetch", {
    configurable: true, enumerable: false, writable: true, value: __quenchBlobFetch,
  });
}
"#);
