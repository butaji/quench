//! Polyfill: `globals-extra`

const WEB_API_START: &str = "/* quench:web-api:start */";
const WEB_API_END: &str = "/* quench:web-api:end */";

/// The Web API constructors installed by `globals-extra`, as a projection of
/// the same checked source used by the legacy installer.
pub fn web_api_source() -> &'static str {
    let source = JS
        .split_once(WEB_API_START)
        .expect("globals-extra Web API source start marker")
        .1;
    source
        .split_once(WEB_API_END)
        .expect("globals-extra Web API source end marker")
        .0
}

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
/* quench:web-api:start */
/* quench:web-api:end */
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
