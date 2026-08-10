//! Polyfill: `globals-extra`

pub const JS: &str = r#"const __nodeDomExceptionCodes = {
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
"#;
