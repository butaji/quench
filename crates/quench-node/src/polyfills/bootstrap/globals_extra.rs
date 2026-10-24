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
if (typeof RegExp.escape !== "function") {
  Object.defineProperty(RegExp, "escape", {
    configurable: true,
    writable: true,
    value: function escape(value) {
      const string = `${value}`;
      let result = "";
      for (let index = 0; index < string.length; index++) {
        const character = string[index];
        const code = string.charCodeAt(index);
        if (index === 0 && /[0-9A-Za-z]/.test(character)) {
          result += `\\x${code.toString(16).padStart(2, "0")}`;
        } else if ("^$\\.*+?()[]{}|/".includes(character)) {
          result += `\\${character}`;
        } else if (",-=<>#&!%:;@~'\"`".includes(character)) {
          result += `\\x${code.toString(16).padStart(2, "0")}`;
        } else if (character === "\n") {
          result += "\\n";
        } else if (character === "\r") {
          result += "\\r";
        } else if (character === "\t") {
          result += "\\t";
        } else if (character === "\v") {
          result += "\\v";
        } else if (character === "\f") {
          result += "\\f";
        } else if (character === " ") {
          result += "\\x20";
        } else if (code >= 0xd800 && code <= 0xdbff) {
          const next = string.charCodeAt(index + 1);
          if (next >= 0xdc00 && next <= 0xdfff) {
            result += character + string[++index];
          } else {
            result += `\\u${code.toString(16).padStart(4, "0")}`;
          }
        } else if (code >= 0xdc00 && code <= 0xdfff) {
          result += `\\u${code.toString(16).padStart(4, "0")}`;
        } else if (/\s/u.test(character)) {
          result += `\\u${code.toString(16).padStart(4, "0")}`;
        } else {
          result += character;
        }
      }
      return result;
    },
  });
}
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
if (typeof globalThis.URLSearchParams !== "function") {
  const decodeFormComponent = (value) => {
    try {
      return decodeURIComponent(value.replace(/\+/g, " "));
    } catch (_) {
      // Form decoding is forgiving: malformed percent escapes are retained.
      return value.replace(/\+/g, " ").replace(/%([0-9a-f]{2})/gi, (_, hex) =>
        String.fromCharCode(parseInt(hex, 16)));
    }
  };
  const encodeFormComponent = (value) => encodeURIComponent(value)
    .replace(/[!'()~]/g, (character) => `%${character.charCodeAt(0).toString(16).toUpperCase()}`)
    .replace(/%20/g, "+");
  const asUSVString = (value) => {
    if (typeof value === "symbol") throw new TypeError("Cannot convert a Symbol value to a string");
    const text = String(value);
    let result = "";
    for (let index = 0; index < text.length; index++) {
      const unit = text.charCodeAt(index);
      if (unit >= 0xD800 && unit <= 0xDBFF) {
        const next = text.charCodeAt(index + 1);
        if (next >= 0xDC00 && next <= 0xDFFF) result += text[index] + text[++index];
        else result += "\uFFFD";
      } else if (unit >= 0xDC00 && unit <= 0xDFFF) result += "\uFFFD";
      else result += text[index];
    }
    return result;
  };
  const checkThis = (receiver) => {
    if (!receiver || !Array.isArray(receiver._pairs)) {
      throw Object.assign(new TypeError('Value of "this" must be of type URLSearchParams'), { code: "ERR_INVALID_THIS" });
    }
  };
  const requireArgs = (args, count, names) => {
    if (args.length < count) {
      throw Object.assign(new TypeError(`The ${names} argument${count > 1 ? "s" : ""} must be specified`), { code: "ERR_MISSING_ARGS" });
    }
  };
  const tupleError = () => Object.assign(
    new TypeError("Each query pair must be an iterable [name, value] tuple"),
    { code: "ERR_INVALID_TUPLE" },
  );
  const iterableError = () => Object.assign(
    new TypeError("Query pairs must be iterable"),
    { code: "ERR_ARG_NOT_ITERABLE" },
  );
  class URLSearchParams {
    constructor(init = undefined) {
      this._pairs = [];
      if (init === undefined) return;
      if (typeof init === "string") {
        const query = init.startsWith("?") ? init.slice(1) : init;
        for (const part of query.split("&")) {
          if (part === "") continue;
          const equals = part.indexOf("=");
          this._pairs.push([
            decodeFormComponent(equals < 0 ? part : part.slice(0, equals)),
            decodeFormComponent(equals < 0 ? "" : part.slice(equals + 1)),
          ]);
        }
        return;
      }
      if (init !== null && (typeof init === "object" || typeof init === "function")) {
        const iterator = init[Symbol.iterator];
        if (iterator !== undefined && iterator !== null) {
          if (typeof iterator !== "function") throw iterableError();
          for (const pair of init) {
            if (pair === null || pair === undefined) throw tupleError();
            let pairIterator;
            try { pairIterator = pair[Symbol.iterator]; } catch (_) { throw tupleError(); }
            if (typeof pairIterator !== "function") throw tupleError();
            const values = [...pair];
            if (values.length !== 2) throw tupleError();
            this._pairs.push([asUSVString(values[0]), asUSVString(values[1])]);
          }
          return;
        }
        if (iterator !== undefined && iterator !== null) throw iterableError();
        for (const name of Reflect.ownKeys(init)) {
          if (typeof name === "symbol") throw new TypeError("Cannot convert a Symbol value to a string");
          if (Object.prototype.propertyIsEnumerable.call(init, name)) this._pairs.push([name, asUSVString(init[name])]);
        }
        return;
      }
      const text = asUSVString(init);
      if (text !== "") this._pairs.push([text, ""]);
    }
    get size() { checkThis(this); return this._pairs.length; }
    append(name, value) {
      checkThis(this); requireArgs(arguments, 2, '"name" and "value"');
      this._pairs.push([asUSVString(name), asUSVString(value)]);
      this._onchange?.();
    }
    delete(name, value) {
      checkThis(this); requireArgs(arguments, 1, '"name"');
      name = asUSVString(name);
      const hasValue = arguments.length > 1;
      value = hasValue ? asUSVString(value) : undefined;
      this._pairs = this._pairs.filter((pair) => pair[0] !== name || (hasValue && pair[1] !== value));
      this._onchange?.();
    }
    get(name) {
      checkThis(this); requireArgs(arguments, 1, '"name"');
      name = asUSVString(name);
      const pair = this._pairs.find((item) => item[0] === name);
      return pair === undefined ? null : pair[1];
    }
    getAll(name) {
      checkThis(this); requireArgs(arguments, 1, '"name"');
      name = asUSVString(name);
      return this._pairs.filter((item) => item[0] === name).map((item) => item[1]);
    }
    has(name, value) {
      checkThis(this); requireArgs(arguments, 1, '"name"');
      name = asUSVString(name);
      if (arguments.length > 1) {
        value = asUSVString(value);
        return this._pairs.some((item) => item[0] === name && item[1] === value);
      }
      return this._pairs.some((item) => item[0] === name);
    }
    set(name, value) {
      checkThis(this); requireArgs(arguments, 2, '"name" and "value"');
      name = asUSVString(name);
      value = asUSVString(value);
      let found = false;
      const next = [];
      for (const pair of this._pairs) {
        if (pair[0] !== name) next.push(pair);
        else if (!found) { next.push([name, value]); found = true; }
      }
      if (!found) next.push([name, value]);
      this._pairs = next;
      this._onchange?.();
    }
    sort() {
      checkThis(this);
      this._pairs = this._pairs.map((pair, index) => ({ pair, index }))
        .sort((a, b) => a.pair[0] < b.pair[0] ? -1 : a.pair[0] > b.pair[0] ? 1 : a.index - b.index)
        .map((item) => item.pair);
      this._onchange?.();
    }
    entries() { checkThis(this); return makeIterator(this._pairs, 0); }
    keys() { checkThis(this); return makeIterator(this._pairs, 1); }
    values() { checkThis(this); return makeIterator(this._pairs, 2); }
    forEach(callback, thisArg = undefined) {
      checkThis(this);
      if (typeof callback !== "function") {
        throw Object.assign(new TypeError("The \"callbackFn\" argument must be of type function"), { code: "ERR_INVALID_ARG_TYPE" });
      }
      for (const [name, value] of this._pairs.slice()) callback.call(thisArg, value, name, this);
    }
    toString() {
      checkThis(this);
      return this._pairs.map(([name, value]) => `${encodeFormComponent(name)}=${encodeFormComponent(value)}`).join("&");
    }
  }
  const URLSearchParamsIteratorPrototype = Object.create(Object.prototype);
  Object.defineProperty(URLSearchParamsIteratorPrototype, Symbol.toStringTag, {
    value: "URLSearchParams Iterator",
  });
  const makeIterator = (pairs, kind) => {
    let index = 0;
    const iterator = {
      next() {
        if (this !== iterator) throw Object.assign(new TypeError('Value of "this" must be of type URLSearchParamsIterator'), { code: "ERR_INVALID_THIS" });
        if (index >= pairs.length) return { value: undefined, done: true };
        const pair = pairs[index++];
        return { value: kind === 0 ? pair.slice() : pair[kind - 1], done: false };
      },
      [Symbol.iterator]() { return this; },
      [Symbol.for("nodejs.util.inspect.custom")](depth, options) {
        if (depth < 0) return "[Object]";
        const values = pairs.slice(index).map((pair) => {
          const value = kind === 0 ? pair : pair[kind - 1];
          return kind === 0 ? `[ '${value[0]}', '${value[1]}' ]` : `'${value}'`;
        });
        if (values.length === 0) return "URLSearchParams Iterator {  }";
        const flat = `URLSearchParams Iterator { ${values.join(", ")} }`;
        if (flat.length <= (options?.breakLength ?? 80)) return flat;
        return `URLSearchParams Iterator {\n  ${values.join(",\n  ")} }`;
      },
    };
    Object.setPrototypeOf(iterator, URLSearchParamsIteratorPrototype);
    return iterator;
  };
  Object.defineProperty(URLSearchParams.prototype, "entries", { enumerable: true });
  Object.defineProperty(URLSearchParams.prototype, "keys", { enumerable: true });
  Object.defineProperty(URLSearchParams.prototype, "values", { enumerable: true });
  URLSearchParams.prototype[Symbol.iterator] = URLSearchParams.prototype.entries;
  Object.defineProperty(URLSearchParams.prototype, "size", { enumerable: true, configurable: true });
  Object.defineProperty(URLSearchParams.prototype, Symbol.toStringTag, { value: "URLSearchParams" });
  Object.defineProperty(URLSearchParams.prototype, Symbol.for("nodejs.util.inspect.custom"), {
    configurable: true,
    value(depth, options) {
      checkThis(this);
      if (depth < 0) return "[Object]";
      if (this._pairs.length === 0) return "URLSearchParams {}";
      const values = this._pairs.map(([name, value]) => `'${name}' => '${value}'`);
      const flat = `URLSearchParams { ${values.join(", ")} }`;
      if (flat.length <= (options?.breakLength ?? 80)) return flat;
      return `URLSearchParams {\n  ${values.join(",\n  ")} }`;
    },
  });
  Object.defineProperty(globalThis, "URLSearchParams", {
    configurable: true, enumerable: false, writable: true, value: URLSearchParams,
  });
}
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
  writable: false,
  value: __nodeBlobEnsureReadable,
});
if (typeof globalThis.Blob !== "function" ||
    typeof globalThis.Blob.prototype?.arrayBuffer !== "function") {
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
    [Symbol.for("nodejs.util.inspect.custom")](depth) {
      if (depth < 0) return "[Blob]";
      const type = this.type.replace(/\\/g, "\\\\").replace(/'/g, "\\'");
      return `Blob { size: ${this.size}, type: '${type}' }`;
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
        type: "bytes",
        pull(controller) {
          __nodeBlobEnsureReadable(thisBlob);
          while (index < chunks.length) {
            const chunk = chunks[index++];
            if (chunk.byteLength === 0) continue;
            // Byte stream enqueue detaches its input view. Copy from Blob's
            // retained bytes so reading the stream does not detach its data.
            controller.enqueue(new Uint8Array(chunk));
            return;
          }
          controller.close();
          // A pending BYOB read needs an explicit zero-byte response to
          // complete after the byte stream closes.
          controller.byobRequest?.respond(0);
        }
      }, { highWaterMark: 0 });
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
        if (name === "text" || name === "bytes" || name === "arrayBuffer") {
          try {
            globalThis.__quenchBlobEnsureReadable?.(this);
          } catch (error) {
            return Promise.reject(error);
          }
        }
        // The embedded Blob fallback stores bytes in `_data`; resolve these
        // methods explicitly because async class methods can lose their
        // receiver across the VM's Promise continuation boundary.
        if (this._data && (name === "text" || name === "bytes" || name === "arrayBuffer")) {
          if (name === "text") return Promise.resolve(this._data.toString());
          if (name === "bytes") return Promise.resolve(new Uint8Array(this._data));
          const data = this._data;
          return Promise.resolve(data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength));
        }
        const result = original.apply(this, args);
        if (name === "stream" && typeof this["\0quench:file-backed:path"] === "string" &&
            typeof result?.getReader === "function" && typeof globalThis.ReadableStream === "function") {
          const blob = this;
          const reader = result.getReader();
          return new globalThis.ReadableStream({
            async pull(controller) {
              try {
                globalThis.__quenchBlobEnsureReadable(blob);
                const item = await reader.read();
                if (item.done) controller.close();
                else controller.enqueue(item.value);
              } catch (error) {
                controller.error(error);
              }
            },
            cancel(reason) { return reader.cancel(reason); },
          }, { highWaterMark: 0 });
        }
        return result;
      },
    });
  }
}
if (typeof globalThis.File !== "function" && typeof globalThis.Blob === "function") {
  const fileStates = new WeakMap();
  const invalidFileThis = () => Object.assign(new TypeError("Illegal invocation"), {
    code: "ERR_INVALID_THIS",
  });
  class File extends globalThis.Blob {
    constructor(parts, name, options = {}) {
      if (arguments.length < 2) {
        throw Object.assign(new TypeError('The "fileBits" and "fileName" arguments are required'), {
          code: "ERR_MISSING_ARGS",
        });
      }
      super(parts, options);
      let lastModified = options?.lastModified;
      if (lastModified === undefined) lastModified = Date.now();
      else {
        lastModified = +lastModified;
        if (Number.isNaN(lastModified)) lastModified = 0;
      }
      let fileName = `${name}`;
      if (typeof fileName.toWellFormed === "function") fileName = fileName.toWellFormed();
      fileStates.set(this, { name: fileName, lastModified });
    }
    get name() {
      const state = fileStates.get(this);
      if (!state) throw invalidFileThis();
      return state.name;
    }
    get lastModified() {
      const state = fileStates.get(this);
      if (!state) throw invalidFileThis();
      return state.lastModified;
    }
    [Symbol.for("nodejs.util.inspect.custom")]() {
      return `File { size: ${this.size}, type: '${this.type}', name: '${this.name}', lastModified: ${this.lastModified} }`;
    }
  }
  Object.defineProperty(File.prototype, Symbol.toStringTag, {
    configurable: true,
    value: "File",
  });
  for (const name of ["name", "lastModified"]) {
    const descriptor = Object.getOwnPropertyDescriptor(File.prototype, name);
    Object.defineProperty(File.prototype, name, {
      ...descriptor,
      enumerable: true,
    });
  }
  Object.defineProperty(globalThis, "File", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: File
  });
}
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
