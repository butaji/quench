const __nodeUtilFormatColor = (value) => {
  if (value === null) return "\u001b[1mnull\u001b[22m";
  if (value === undefined) return "\u001b[90mundefined\u001b[39m";
  if (
    typeof value === "boolean" ||
    typeof value === "number" ||
    typeof value === "bigint"
  )
    return `\u001b[33m${String(value)}${typeof value === "bigint" ? "n" : ""}\u001b[39m`;
  if (typeof value === "symbol") return `\u001b[32m${String(value)}\u001b[39m`;
  return String(value);
};
const __nodeUtilFormatCompact = (options, args) => {
  if (
    options.compact === undefined ||
    args[0] !== "%s" ||
    !Array.isArray(args[1])
  )
    return null;
  return `[ ${args[1]
    .map((value) =>
      value && typeof value === "object" ? "[Object]" : String(value)
    )
    .join(", ")} ]`;
};
globalThis.__nodeUtil.formatWithOptions = (options, ...args) => {
  if (!options || typeof options !== "object" || Array.isArray(options)) {
    const error = new TypeError(
      'The "inspectOptions" argument must be an object'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options.colors && typeof args[0] !== "string")
    return args.map(__nodeUtilFormatColor).join(" ");
  const compact = __nodeUtilFormatCompact(options, args);
  if (compact !== null) return compact;
  const previous =
    globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator;
  if (options && options.numericSeparator !== undefined)
    globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator =
      options.numericSeparator;
  try {
    return globalThis.__nodeUtil.format(...args);
  } finally {
    globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator = previous;
  }
};
const __nodeQuerystringEscape = (value) => encodeURIComponent(String(value));
const __nodeQuerystringDecodeByte = (input, index, decodeSpaces) => {
  if (input[index] === "+" && decodeSpaces)
    return { bytes: [0x20], advance: 0 };
  if (
    input[index] === "%" &&
    /^[0-9a-f]{2}$/i.test(input.slice(index + 1, index + 3))
  )
    return {
      bytes: [parseInt(input.slice(index + 1, index + 3), 16)],
      advance: 2
    };
  let character = input[index];
  if (
    input.charCodeAt(index) >= 0xd800 &&
    input.charCodeAt(index) <= 0xdbff &&
    index + 1 < input.length &&
    input.charCodeAt(index + 1) >= 0xdc00 &&
    input.charCodeAt(index + 1) <= 0xdfff
  )
    character += input[index + 1];
  return {
    bytes: Array.from(new TextEncoder().encode(character)),
    advance: character.length - 1
  };
};
const __nodeQuerystringParseValue = (value, options) => {
  const inputValue = String(value).replace(/\+/g, " ");
  if (options && options.decodeURIComponent) {
    try {
      return options.decodeURIComponent(inputValue);
    } catch (_) {}
  }
  return globalThis.__nodeQuerystring.unescape(inputValue);
};
const __nodeQuerystringAddValue = (result, key, value) => {
  result[key] =
    result[key] === undefined
      ? value
      : Array.isArray(result[key])
        ? result[key].concat(value)
        : [result[key], value];
};
const __nodeQuerystringExports = {
  escape: __nodeQuerystringEscape,
  unescape: (value) =>
    globalThis.__nodeQuerystring.unescapeBuffer(String(value), true).toString(),
  unescapeBuffer: (value, decodeSpaces = false) => {
    const input = String(value);
    const bytes = [];
    for (let i = 0; i < input.length; i++) {
      const decoded = __nodeQuerystringDecodeByte(input, i, decodeSpaces);
      bytes.push(...decoded.bytes);
      i += decoded.advance;
    }
    return NodeBuffer.from(new Uint8Array(bytes));
  },
  stringify: (object, sep = "&", eq = "=", options = {}) => {
    if (!object || typeof object !== "object") return "";
    sep = sep == null ? "&" : String(sep);
    eq = eq == null ? "=" : String(eq);
    const encode =
      options && options.encodeURIComponent
        ? options.encodeURIComponent
        : globalThis.__nodeQuerystring.escape;
    return Object.keys(object)
      .flatMap((key) => {
        const value = object[key];
        if (Array.isArray(value) && value.length === 0) return [];
        const values = Array.isArray(value) ? value : [value];
        return values.map((item) => {
          let encodedKey;
          let encodedValue;
          try {
            encodedKey = encode(key);
            encodedValue =
              item === null ||
              typeof item === "object" ||
              typeof item === "function" ||
              (typeof item === "number" && !Number.isFinite(item))
                ? ""
                : encode(item);
          } catch (error) {
            if (error instanceof URIError) {
              error.code = "ERR_INVALID_URI";
              error.message = "URI malformed";
            }
            throw error;
          }
          return `${encodedKey}${eq}${encodedValue}`;
        });
      })
      .join(sep);
  },
  parse: (input, sep = "&", eq = "=", options = {}) => {
    const result = Object.create(null);
    if (input == null || input === "") return result;
    const separator = sep == null ? "&" : String(sep);
    const equals = eq == null ? "=" : String(eq);
    const maxKeys =
      options && options.maxKeys !== undefined
        ? options.maxKeys === 0
          ? Infinity
          : Number(options.maxKeys)
        : 1000;
    String(input)
      .split(separator)
      .slice(0, maxKeys)
      .filter(Boolean)
      .forEach((part) => {
        const index = part.indexOf(equals);
        const key = __nodeQuerystringParseValue(
          index < 0 ? part : part.slice(0, index),
          options
        );
        const value = __nodeQuerystringParseValue(
          index < 0 ? "" : part.slice(index + equals.length),
          options
        );
        __nodeQuerystringAddValue(result, key, value);
      });
    return result;
  }
};
let __nodeQuerystringInstance;
globalThis.__nodeQuerystringInitialized = false;
globalThis.__nodeQuerystring = new Proxy(
  {},
  {
    get: (_, key) => {
      globalThis.__nodeQuerystringInitialized = true;
      __nodeQuerystringInstance ||= __nodeQuerystringExports;
      return __nodeQuerystringInstance[key];
    },
    ownKeys: () => Reflect.ownKeys(__nodeQuerystringExports),
    getOwnPropertyDescriptor: (_, key) => ({
      enumerable: true,
      configurable: true,
      value: __nodeQuerystringExports[key]
    }),
    set: (_, key, value) => {
      __nodeQuerystringExports[key] = value;
      return true;
    }
  }
);
class NodeURLSearchParams {
  constructor(init = "") {
    this._pairs = [];
    if (typeof init === "string") {
      init
        .replace(/^\?/, "")
        .split("&")
        .filter(Boolean)
        .forEach((part) => {
          const i = part.indexOf("=");
          this.append(
            decodeURIComponent(i < 0 ? part : part.slice(0, i)),
            decodeURIComponent(i < 0 ? "" : part.slice(i + 1))
          );
        });
    } else Object.keys(init).forEach((key) => this.append(key, init[key]));
  }
  append(key, value) {
    this._pairs.push([String(key), String(value)]);
  }
  set(key, value) {
    this.delete(key);
    this.append(key, value);
  }
  get(key) {
    const pair = this._pairs.find(([name]) => name === String(key));
    return pair ? pair[1] : null;
  }
  getAll(key) {
    return this._pairs
      .filter(([name]) => name === String(key))
      .map(([, value]) => value);
  }
  has(key) {
    return this._pairs.some(([name]) => name === String(key));
  }
  delete(key) {
    this._pairs = this._pairs.filter(([name]) => name !== String(key));
  }
  toString() {
    return this._pairs
      .map(
        ([key, value]) =>
          `${encodeURIComponent(key)}=${encodeURIComponent(value)}`
      )
      .join("&");
  }
}
globalThis.__nodeURLSearchParams = NodeURLSearchParams;
globalThis.__nodeURL = class NodeURL {
  constructor(input, base) {
    let value = String(input);
    if (base && !/^[a-z][a-z0-9+.-]*:/.test(value)) {
      const baseUrl = new NodeURL(base);
      value = value.startsWith("/")
        ? baseUrl.origin + value
        : baseUrl.origin + baseUrl.pathname.replace(/\/[^/]*$/, "/") + value;
    }
    const match = value.match(
      /^([a-z][a-z0-9+.-]*:)?(?:\/\/([^/?#]*))?([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/i
    );
    if (!match) throw new TypeError("Invalid URL");
    this.protocol = match[1] || "";
    this.host = match[2] || "";
    this.hostname = this.host.replace(/^.*@/, "").split(":")[0];
    this.port = this.host.includes(":")
      ? this.host.slice(this.host.lastIndexOf(":") + 1)
      : "";
    this.pathname = match[3] || "/";
    this.search = match[4] ? `?${match[4]}` : "";
    this.hash = match[5] ? `#${match[5]}` : "";
    this.origin =
      this.protocol && this.host ? `${this.protocol}//${this.host}` : "null";
    this.searchParams = new NodeURLSearchParams(match[4] || "");
  }
  get href() {
    const query = this.searchParams.toString();
    const prefix =
      this.protocol === "file:"
        ? "file://"
        : this.origin === "null"
          ? ""
          : this.origin;
    return `${prefix}${this.pathname}${query ? `?${query}` : this.search}${this.hash}`;
  }
  toString() {
    return this.href;
  }
};
globalThis.URL = globalThis.__nodeURL;
globalThis.URLSearchParams = globalThis.__nodeURLSearchParams;
const __nodeUrlModuleExports = {
  URL: globalThis.__nodeURL,
  URLSearchParams: globalThis.__nodeURLSearchParams,
  fileURLToPath: (value) => {
    let href;
    if (value && typeof value.href === "string") href = value.href;
    else href = String(value);
    if (!href.startsWith("file://"))
      throw new TypeError("URL must be a file URL");
    let p = decodeURIComponent(href.slice("file://".length));
    return p || "/";
  },
  pathToFileURL: (value, options) => {
    const windows = options && options.windows;
    const sep = windows ? "\\" : "/";
    const resolved = globalThis.__nodePath.resolve(String(value));
    const isAbsolute = globalThis.__nodePath.isAbsolute(resolved);
    let p = resolved.split(sep).join("/");
    if (windows && /^[A-Za-z]:/.test(p)) p = "/" + p;
    const trailing = resolved.endsWith(sep) || resolved.endsWith("/");
    p = p
      .split("/")
      .map((seg, i) => {
        if (i === 0) return seg;
        return encodeURIComponent(seg).replace(
          /[!'()*]/g,
          (c) => "%" + c.charCodeAt(0).toString(16).toUpperCase()
        );
      })
      .join("/");
    if (trailing && !p.endsWith("/")) p = p + "/";
    return new globalThis.__nodeURL("file://" + (isAbsolute ? "" : "") + p);
  },
  parse: (value) => {
    if (typeof value !== "string") {
      let received;
      if (value == null) received = String(value);
      else {
        try {
          received = `type ${typeof value} (${String(value)})`;
        } catch (_) {
          received = `type ${typeof value} (${Object.prototype.toString.call(value)})`;
        }
      }
      const error = new TypeError(
        'The "url" argument must be of type string.' + ` Received ${received}`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (/^https?:\/\/[^/]*:[.]|^git\+ssh:\/\/[^/]*:[^/]+\/[^/]+/.test(value)) {
      const error = new TypeError("Invalid URL");
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    if (/%(?:[0-9A-Fa-f]{0,1}|[0-9A-Fa-f]{2})/.test(value)) {
      try {
        decodeURIComponent(value);
      } catch (_) {
        throw new URIError("URI malformed");
      }
    }
    const rawAuthority =
      value.match(/^[a-z][a-z0-9+.-]*:\/\/([^/]+)/i)?.[1] || "";
    if (/\u0000/.test(rawAuthority) || /[#%/?@[\\\]^|]/.test(rawAuthority)) {
      const error = new TypeError("Invalid URL");
      error.code = "ERR_INVALID_URL";
      error.input = value;
      throw error;
    }
    let input = value.trim();
    if (/^[a-z][a-z0-9+.-]*:/.test(input)) input = input.replaceAll("\\", "/");
    const parsed = new globalThis.__nodeURL(input);
    const protocol = parsed.protocol.toLowerCase();
    const authority =
      input.match(/^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i)?.[1] || "";
    const at = authority.lastIndexOf("@");
    const auth = at >= 0 ? decodeURIComponent(authority.slice(0, at)) : null;
    const host = (at >= 0 ? authority.slice(at + 1) : authority).toLowerCase();
    const hostname = host.replace(/^\[|\]$/g, "").split(":")[0];
    const port = host.match(/:(\d+)$/)?.[1] || null;
    const pathname = parsed.pathname || (host ? "/" : "");
    const search = parsed.search || null;
    return {
      protocol: protocol || null,
      slashes: protocol ? true : null,
      auth,
      host: host || null,
      port,
      hostname: hostname || null,
      hash: parsed.hash || null,
      search,
      query: search ? search.slice(1) : null,
      pathname: pathname || null,
      path: pathname ? `${pathname}${search || ""}` : null,
      href: `${protocol}${host ? `//${host}` : ""}${pathname}${search || ""}${parsed.hash || ""}`
    };
  },
  format: (value) => {
    if (value instanceof globalThis.__nodeURL) return value.href;
    if (typeof value === "string") {
      try {
        const href = new globalThis.__nodeURL(value).href;
        return value.endsWith("?") && !href.endsWith("?") ? `${href}?` : href;
      } catch (_) {
        return value;
      }
    }
    if (value && typeof value === "object") {
      const protocol = value.protocol || "";
      const host = value.host || value.hostname || "";
      const prefix =
        protocol && (value.slashes || host) ? `${protocol}//` : protocol;
      const pathname = value.pathname || (host ? "/" : "");
      let search = value.search;
      if (search === undefined && value.query !== undefined) {
        search = typeof value.query === "string" ? `?${value.query}` : "";
      }
      return `${prefix}${host}${pathname}${search || ""}${value.hash || ""}`;
    }
    return String(value);
  },
  resolve: (from, to) => new globalThis.__nodeURL(to, from).href
};
let __nodeUrlModuleInstance;
globalThis.__nodeUrlInitialized = false;
globalThis.__nodeUrlModule = new Proxy(
  {},
  {
    get: (_, key) => {
      globalThis.__nodeUrlInitialized = true;
      __nodeUrlModuleInstance ||= __nodeUrlModuleExports;
      return __nodeUrlModuleInstance[key];
    },
    ownKeys: () => Reflect.ownKeys(__nodeUrlModuleExports),
    getOwnPropertyDescriptor: (_, key) => ({
      enumerable: true,
      configurable: true,
      value: __nodeUrlModuleExports[key]
    })
  }
);
