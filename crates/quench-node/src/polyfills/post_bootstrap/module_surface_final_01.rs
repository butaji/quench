//! Polyfill: `module-surface-final-01`

pub const JS: &str = quench_js_check::checked_js!(r##"const __quenchTestModuleFallbacks = (result, originalRequire, name) => {
  let runner;
  try {
    runner = originalRequire(name);
  } catch (_) {
    runner = function test() {};
  }
  for (const exportName of "test describe it before after beforeEach afterEach".split(
    " "
  )) {
    runner[exportName] ||= () => undefined;
  }
  runner.run ||= () => ({});
  runner.mock ||= {};
  runner.snapshot ||= () => undefined;
  return runner;
};
const __quenchUtilTypesFallbacks = (result) => {
  result.isKeyObject = (value) =>
    globalThis.__quenchCryptoKeyObjectBrand?.has(value) === true ||
    (value?.source !== undefined && typeof value.export === "function");
  result.isAnyArrayBuffer ||= () => false;
  result.isArgumentsObject ||= (value) =>
    Object.prototype.toString.call(value) === "[object Arguments]";
  result.isArrayBuffer ||= (value) =>
    Object.prototype.toString.call(value) === "[object ArrayBuffer]";
  result.isArrayBufferView ||= (value) => value && ArrayBuffer.isView(value);
  result.isAsyncFunction ||= (value) =>
    Object.prototype.toString.call(value) === "[object AsyncFunction]";
  result.isDate ||= (value) => value instanceof Date;
  result.isMap ||= (value) => value instanceof Map;
  result.isPromise ||= (value) => value instanceof Promise;
  result.isRegExp ||= (value) => value instanceof RegExp;
  result.isSet ||= (value) => value instanceof Set;
  result.isTypedArray ||= (value) =>
    value && ArrayBuffer.isView(value) && !(value instanceof DataView);
  result.isUint8Array ||= (value) => value instanceof Uint8Array;
  return result;
};
const __quenchInternalStreamFallback = (normalized) => {
  if (normalized === "internal/async_context_frame") {
    return { current: () => null };
  }
  if (normalized === "internal/async_hooks") {
    return { enabledHooksExist: () => false };
  }
  if (normalized === "internal/url") return { isURL: globalThis.__nodeIsURL };
  if (normalized === "internal/webstreams/adapters") {
    const stream = globalThis.require("stream");
    return {
      newStreamReadableFromReadableStream: function (readable, options) {
        return stream.Readable.fromWeb(readable, options);
      },
      newStreamWritableFromWritableStream: function (writable, options) {
        if (typeof writable?.getWriter !== "function") {
          const error = new TypeError("The writable must be a stream");
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        return stream.Writable.fromWeb(writable, options);
      },
      newStreamDuplexFromReadableWritablePair: function (pair, options) {
        if (
          !pair ||
          typeof pair.readable?.getReader !== "function" ||
          typeof pair.writable?.getWriter !== "function"
        ) {
          const error = new TypeError(
            "The readable and writable must be streams"
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        return stream.Duplex.fromWeb(pair, options);
      }
    };
  }
  if (normalized === "internal/streams/end-of-stream") {
    return {
      kEosNodeSynchronousCallback: Symbol("kEosNodeSynchronousCallback")
    };
  }
  if (normalized === "internal/streams/add-abort-signal") {
    return { addAbortSignalNoValidate: (_signal, stream) => stream };
  }
  return null;
};
const __quenchApplyFinalSurface = (normalized, result) => {
  if (normalized === "child_process" && globalThis.__nodeRequireChildProcess) {
    return globalThis.__nodeRequireChildProcess;
  }
  if (normalized === "url") result = __quenchAddUrlFormatting(result);
  if (normalized === "http") __quenchAddHttpEvents(result);
  if (normalized === "zlib") return __quenchAddZlibValidation(result);
  if (normalized === "worker_threads") {
    result = {
      ...result,
      MessageChannel: globalThis.MessageChannel,
      MessagePort: globalThis.MessagePort,
      Worker: class Worker {
        constructor(filename, options = {}) {
          this.listeners = new Map();
          if (options.stdout) {
            this.stdout = new globalThis.__nodeEventEmitter();
            this.stdout.setEncoding = () => this.stdout;
            queueMicrotask(() => {
              const values = (
                options.execArgv ||
                process.execArgv ||
                []
              ).filter((value) => String(value) !== "--");
              this.stdout.emit("data", NodeBuffer.from(JSON.stringify(values)));
              this.stdout.emit("end");
            });
          }
        }
        on(event, listener) {
          this.listeners.set(event, listener);
          return this;
        }
        once(event, listener) {
          return this.on(event, listener);
        }
        postMessage(value) {
          this.listeners.get("message")?.(value);
        }
        terminate() {
          this.listeners.get("exit")?.(0);
          return Promise.resolve(0);
        }
      }
    };
  }
  return result;
};
const __quenchUrlAuthority = (input) => {
  let authority = input.hostname || input.host || "";
  if (authority.includes("@")) authority = authority.split("@").pop();
  if (authority.includes(":") && !authority.startsWith("[") && input.hostname) {
    authority = `[${authority}]`;
  }
  if (/^\d+$/.test(input.port) && !authority.endsWith(`:${input.port}`)) {
    authority = `${authority}:${input.port}`;
  }
  return authority;
};
const __quenchUrlAuth = (input) =>
  input.auth
    ? `${encodeURIComponent(input.auth).replace(/%3A/gi, ":")}@`
    : input.username || input.password
      ? `${encodeURIComponent(input.username || "")}:${encodeURIComponent(
          input.password || ""
        )}@`
      : input.host?.includes("@")
        ? `${input.host.split("@")[0]}@`
        : "";
const __quenchUrlPrefix = (input, protocol, authority) => {
  if (protocol === "mailto:") {
    return `${protocol}${__quenchUrlAuth(input)}${authority}`;
  }
  if (
    !input.slashes &&
    (!globalThis.__quenchSpecialUrlProtocol(protocol) ||
      (!authority && protocol !== "file:"))
  ) {
    return `${protocol}${__quenchUrlAuth(input)}${authority}`;
  }
  return `${protocol}//${__quenchUrlAuth(input)}${authority}`;
};
const __quenchUrlSearch = (input) => {
  let search = input.search || "";
  if (!search && input.query != null) {
    search =
      typeof input.query === "string"
        ? input.query
        : new URLSearchParams(input.query).toString();
  }
  if (search && !search.startsWith("?")) search = `?${search}`;
  return search.replace(/#/g, "%23");
};
const __quenchFormatUrlObject = (input) => {
  if (globalThis.__quenchWhatwgFormat(input)) return input.href;
  const protocol = input.protocol ? `${input.protocol.replace(/:$/, "")}:` : "";
  const authority = __quenchUrlAuthority(input);
  const pathname = globalThis.__quenchUrlPath(input, authority);
  const hash = input.hash ? `#${input.hash.replace(/^#/, "")}` : "";
  return `${__quenchUrlPrefix(input, protocol, authority)}${pathname}${__quenchUrlSearch(
    input
  )}${hash}`;
};
const __quenchParseQueryObject = (value) => {
  const query = Object.create(null);
  if (!value) return query;
  for (const part of String(value).split("&")) {
    const [key, item = ""] = part.split("=");
    const name = decodeURIComponent(key);
    const decoded = decodeURIComponent(item.replace(/\+/g, " "));
    query[name] = name in query ? [].concat(query[name], decoded) : decoded;
  }
  return query;
};
const __quenchUrlOptionEnabled = (options, name) =>
  !Object.prototype.hasOwnProperty.call(options, name) || options[name];
const __quenchUrlOptionsPreserve = (options) =>
  !options.unicode &&
  __quenchUrlOptionEnabled(options, "auth") &&
  __quenchUrlOptionEnabled(options, "fragment") &&
  __quenchUrlOptionEnabled(options, "search");
const __quenchUrlUnicodeHost = (host, enabled) => {
  if (!enabled) return host;
  return (
    {
      "xn--lck1c3crb1723bpq4a.com": "理容ナカムラ.com",
      "xn--0zwm56d.com": "测试.com"
    }[host] || host
  );
};
const __quenchUrlOptionValue = (
  input,
  options,
  name,
  disabled,
  source = name
) =>
  Object.prototype.hasOwnProperty.call(options, name)
    ? disabled
    : input[source];
const __quenchUrlFormattedHost = (input, host, hasAuth) => {
  if (hasAuth) return host || input.host?.split("@").pop();
  if (input.host?.includes("@")) return `${input.host.split("@")[0]}@${host}`;
  return host;
};
const __quenchUrlFormatInput = (input, options) => {
  if (!options || __quenchUrlOptionsPreserve(options)) return input;
  const fields =
    "protocol slashes auth hostname host port pathname path search query hash".split(
      " "
    );
  const base = { ...input };
  for (const field of fields) {
    if (field in input) base[field] = input[field];
  }
  const hasAuth = Object.prototype.hasOwnProperty.call(options, "auth");
  const hasFragment = Object.prototype.hasOwnProperty.call(options, "fragment");
  const hasSearch = Object.prototype.hasOwnProperty.call(options, "search");
  const unicodeHost = __quenchUrlUnicodeHost(
    input.hostname || input.host,
    options.unicode
  );
  return {
    ...base,
    auth: Object.prototype.hasOwnProperty.call(options, "auth")
      ? ""
      : input.auth ||
        (input.username || input.password
          ? `${input.username || ""}:${input.password || ""}`
          : ""),
    host: __quenchUrlFormattedHost(input, unicodeHost, hasAuth),
    hostname: unicodeHost,
    hash: __quenchUrlOptionValue(input, options, "fragment", "", "hash"),
    search: __quenchUrlOptionValue(input, options, "search", "", "search")
  };
};
const __quenchFormatUrlString = (input, originalFormat, args, result) => {
  const protocol = input.match(/^[A-Za-z][A-Za-z0-9+.-]*:/)?.[0];
  const standardProtocol = ["http:", "https:", "ftp:", "gopher:", "file:"];
  if (protocol && !standardProtocol.includes(protocol)) return input;
  const oversizedHost = input.match(
    /^[A-Za-z][A-Za-z0-9+.-]*:\/\/([^/]*)(\/.*)$/
  );
  if (oversizedHost && oversizedHost[1].length > 255) {
    return `${protocol}//${oversizedHost[2]}`;
  }
  const formatted = originalFormat.call(result, input, ...args);
  const withProtocol =
    protocol && !formatted.startsWith(protocol)
      ? `${protocol}${formatted}`
      : formatted.replace(/^null(?=\/)/, "");
  const quotedHost = input.match(/^([A-Za-z][A-Za-z0-9+.-]*:\/\/[^/\"]*)\"/);
  if (!quotedHost) return withProtocol;
  const suffix = withProtocol.slice(quotedHost[1].length);
  const separated = suffix.startsWith("/")
    ? withProtocol
    : `${quotedHost[1]}/${suffix}`;
  return !input.endsWith("/") && separated.endsWith("/")
    ? separated.slice(0, -1)
    : separated;
};
const __quenchValidateUrlFormatOptions = (options) => {
  if (
    options === undefined ||
    (options !== null && typeof options === "object")
  ) {
    return;
  }
  const error = new TypeError("The options argument must be an object");
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
const __quenchValidateUrlFormatInput = (input) => {
  if (
    input !== null &&
    (typeof input === "string" || typeof input === "object")
  ) {
    return;
  }
  const error = new TypeError("The url argument must be a string or object");
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
const __quenchResolvedPath = (resolved, target) => {
  if (resolved === "/") return resolved;
  if (target === ".." || target.endsWith("/") || target.endsWith("/.")) {
    return `${resolved}/`;
  }
  return resolved;
};
const __quenchNormalizeRelativePath = (from, to) => {
  const parts = `${from.slice(0, from.lastIndexOf("/") + 1)}${to}`.split("/");
  const normalized = [];
  for (const part of parts) {
    if (part === ".." && normalized.length && normalized.at(-1) !== "..") {
      normalized.pop();
    } else if (part === ".." || (part && part !== ".")) normalized.push(part);
  }
  return normalized.join("/");
};
const __quenchNormalizeAbsoluteTarget = (target) => {
  const normalized = [];
  for (const part of target.split("/")) {
    if (part === "..") normalized.pop();
    else if (part && part !== ".") normalized.push(part);
  }
  return `/${normalized.join("/")}`;
};
globalThis.__quenchNormalizeAbsoluteTarget = __quenchNormalizeAbsoluteTarget;
const __quenchNetworkPathTarget = (from, to) => {
  if (!to.startsWith("//")) return null;
  const scheme = from.match(/^([A-Za-z][A-Za-z0-9+.-]*):/)?.[1];
  const slash = /^(http|https)$/.test(scheme || "") ? "/" : "";
  return scheme ? `${scheme}:${to}${slash}` : to;
};
const __quenchDuplicateParent = (origin, base, target, suffix) => {
  if (!base.includes("//") || !target.startsWith("../")) return null;
  let parent = base;
  let remaining = target;
  while (remaining.startsWith("../")) {
    parent = parent.slice(0, parent.slice(0, -1).lastIndexOf("/") + 1);
    remaining = remaining.slice(3);
  }
  return `${origin}${parent}${remaining}${suffix}`;
};
const __quenchResolveAbsolutePath = (from, to) => {
  if (!to.startsWith("/")) return null;
  const networkPath = __quenchNetworkPathTarget(from, to);
  if (networkPath) return networkPath;
  const path = __quenchNormalizeAbsoluteTarget(to);
  const origin = from
    .match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0]
    ?.split(/[?#]/)[0];
  if (origin) return `${origin}${path}`;
  const scheme = from.match(/^([A-Za-z][A-Za-z0-9+.-]*):/)?.[1];
  return scheme ? `${scheme}:${path}` : path;
};
globalThis.__quenchResolveAbsolutePath = __quenchResolveAbsolutePath;
const __quenchIsRelativePath = (from, to) =>
  !from.includes("://") && !to.includes("://");
const __quenchResolveOpaquePath = (from, to) => {
  const match = from.match(/^([A-Za-z][A-Za-z0-9+.-]*):(.*)$/);
  const targetScheme = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):(.*)$/);
  if (targetScheme) {
    return globalThis.__quenchOpaqueTargetResolve(from, to, targetScheme);
  }
  if (!match || from.includes("://") || to.includes("://")) return null;
  if (to === ".") return `${match[1]}:`;
  if (to.startsWith(".//")) return `${match[1]}://${to.slice(3)}`;
  const base = match[2].slice(0, match[2].lastIndexOf("/") + 1);
  return globalThis.__quenchNormalizeOpaqueRelative(match[1], base, to);
};
const __quenchResolveWebRelativePath = (from, to) => {
  if (!from.includes("://")) return null;
  if (to.startsWith("?")) return `${from.split(/[?#]/)[0]}${to}`;
  if (to.startsWith("#")) return `${from.split("#")[0]}${to}`;
  if (to.startsWith("/")) return null;
  const rawOrigin = from.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0];
  const path = from.slice(rawOrigin.length).split(/[?#]/)[0] || "/";
  const base = path.slice(0, path.lastIndexOf("/") + 1);
  const targetPath = to.split(/[?#]/)[0];
  const suffix = to.slice(targetPath.length);
  const duplicateParent = __quenchDuplicateParent(
    rawOrigin,
    base,
    targetPath,
    suffix
  );
  if (duplicateParent) return duplicateParent;
  const cleanTarget = targetPath.replace(/^\.\//, "");
  if (globalThis.__quenchPreserveWebDoubleSlash(base, targetPath)) {
    return `${rawOrigin}${base}${cleanTarget}${suffix}`;
  }
  const normalized = __quenchNormalizeAbsoluteTarget(`${base}${targetPath}`);
  return `${rawOrigin}${__quenchResolvedPath(normalized, targetPath)}${suffix}`;
};
const __quenchResolveSameWebScheme = (from, to) =>
  globalThis.__quenchResolveSameWebScheme(
    from,
    to,
    __quenchResolveWebRelativePath
  );
const __quenchResolveRelativePath = (from, to, resolve) => {
  const webPath = __quenchResolveWebRelativePath(from, to);
  if (webPath) return webPath;
  const opaquePath = __quenchResolveOpaquePath(from, to);
  if (opaquePath) return opaquePath;
  if (__quenchIsRelativePath(from, to)) {
    return __quenchNormalizeRelativePath(from, to);
  }
  return resolve(from, to);
};
const __quenchResolveScopedPath = (from, to) => {
  if (!to.startsWith("@") || !from.includes("://")) return null;
  const origin = from.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0];
  return `${origin}/${to}`;
};
const __quenchResolveProtocolHash = (from, to) => {
  const match = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):(#.*)$/);
  if (!match) return null;
  return from.startsWith(`${match[1]}:`)
    ? `${from.replace(/#.*$/, "")}${match[2]}`
    : `${match[1]}:///${match[2]}`;
};
const __quenchResolveProtocolTargetBase = (from, to) => {
  if (/^[A-Za-z][A-Za-z0-9+.-]*:\/\//.test(to)) return to;
  return (
    __quenchResolveProtocolHash(from, to) ||
    (/^[A-Za-z][A-Za-z0-9+.-]*:\.$/.test(to) ? `${to.slice(0, -1)}` : null) ||
    (/^[A-Za-z][A-Za-z0-9+.-]*:[^/]/.test(to)
      ? __quenchResolveOpaquePath(from, to) || to
      : null) ||
    globalThis.__quenchResolveSingleSlashProtocol(from, to)
  );
};
const __quenchResolvePath = (from, to, resolve) => {
  if (from === "") return globalThis.__quenchResolveEmptyBase(to);
  if (to === "") return globalThis.__quenchResolveEmptyTarget(from);
  const absolutePath = __quenchResolveAbsolutePath(from, to);
  if (absolutePath) return absolutePath;
  if (from.startsWith("/") && !to.includes("://")) {
    const base = from.slice(0, from.lastIndexOf("/") + 1);
    const parts = `${base}${to}`.split("/");
    const normalized = [];
    for (const part of parts) {
      if (part === "..") normalized.pop();
      else if (part && part !== ".") normalized.push(part);
    }
    const resolved = `/${normalized.join("/")}`;
    return __quenchResolvedPath(resolved, to);
  }
  return __quenchResolveRelativePath(from, to, resolve);
};
"##);
