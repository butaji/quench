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
"#);
