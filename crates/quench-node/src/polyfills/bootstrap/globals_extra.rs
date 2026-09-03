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
  const __nodeBlobPart = (part) => {
    if (typeof part === "string") return Buffer.from(part);
    if (part instanceof ArrayBuffer) return Buffer.from(part);
    if (ArrayBuffer.isView(part)) return Buffer.from(part);
    if (part && part._data && typeof part._data.byteLength === "number") {
      return Buffer.from(part._data);
    }
    return Buffer.from(String(part));
  };
  class Blob {
    constructor(parts = [], options = {}) {
      if (!Array.isArray(parts)) {
        throw Object.assign(
          new TypeError('The "sources" argument must be an instance of Array'),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
      this._data = Buffer.concat(parts.map(__nodeBlobPart));
      this.size = this._data.byteLength;
      const type = String(options?.type || "").toLowerCase();
      this.type = /^[\x20-\x7e]*$/.test(type) ? type : "";
    }
    async arrayBuffer() {
      return this._data.buffer.slice(
        this._data.byteOffset,
        this._data.byteOffset + this._data.byteLength
      );
    }
    async text() {
      return this._data.toString();
    }
    slice(start = 0, end = this.size, type = "") {
      const normalize = (value, fallback) => {
        const number = Number(value);
        if (!Number.isFinite(number)) return fallback;
        return number < 0 ? Math.max(this.size + number, 0) : Math.min(number, this.size);
      };
      return new Blob([
        this._data.subarray(normalize(start, 0), normalize(end, this.size))
      ], { type });
    }
    stream() {
      const data = this._data;
      return new __quenchReadableStream({
        start(controller) {
          controller.enqueue(data);
          controller.close();
        }
      });
    }
  }
  for (const name of ["arrayBuffer", "text", "slice", "stream"]) {
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
