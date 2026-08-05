const __quenchTestModuleFallbacks = (result, originalRequire, name) => {
  let runner;
  try {
    runner = originalRequire(name);
  } catch (_) {
    runner = function test() {};
  }
  for (const exportName of [
    "test",
    "describe",
    "it",
    "before",
    "after",
    "beforeEach",
    "afterEach"
  ])
    runner[exportName] ||= () => undefined;
  runner.run ||= () => ({});
  runner.mock ||= {};
  runner.snapshot ||= () => undefined;
  return runner;
};
const __quenchUtilTypesBasicFallbacks = (result) => {
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
};
const __quenchUtilTypesCollectionFallbacks = (result) => {
  result.isDate ||= (value) => value instanceof Date;
  result.isMap ||= (value) => value instanceof Map;
  result.isPromise ||= (value) => value instanceof Promise;
  result.isRegExp ||= (value) => value instanceof RegExp;
  result.isSet ||= (value) => value instanceof Set;
};
const __quenchUtilTypesTypedFallbacks = (result) => {
  result.isTypedArray ||= (value) =>
    value && ArrayBuffer.isView(value) && !(value instanceof DataView);
  result.isUint8Array ||= (value) => value instanceof Uint8Array;
};
const __quenchUtilTypesFallbacks = (result) => {
  (__quenchUtilTypesBasicFallbacks(result),
    __quenchUtilTypesCollectionFallbacks(result));
  return (__quenchUtilTypesTypedFallbacks(result), result);
};
const __quenchInternalStreamFallback = (normalized) => {
  if (normalized === "internal/url") return { isURL: globalThis.__nodeIsURL };
  if (normalized === "internal/streams/end-of-stream")
    return {
      kEosNodeSynchronousCallback: Symbol("kEosNodeSynchronousCallback")
    };
  if (normalized === "internal/streams/add-abort-signal")
    return { addAbortSignalNoValidate: (_signal, stream) => stream };
  return null;
};
const __quenchApplyFinalSurface = (normalized, result) => {
  if (normalized === "url") result = __quenchAddUrlFormatting(result);
  if (normalized === "http") __quenchAddHttpEvents(result);
  if (normalized === "zlib") return __quenchAddZlibValidation(result);
  if (normalized === "worker_threads") {
    result = {
      ...result,
      Worker: class Worker {
        constructor() {
          this.listeners = new Map();
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
  if (authority.includes(":") && !authority.startsWith("[") && input.hostname)
    authority = `[${authority}]`;
  if (/^\d+$/.test(input.port) && !authority.endsWith(`:${input.port}`))
    authority = `${authority}:${input.port}`;
  return authority;
};
const __quenchUrlAuth = (input) =>
  input.auth
    ? `${encodeURIComponent(input.auth).replace(/%3A/gi, ":")}@`
    : input.host?.includes("@")
      ? `${input.host.split("@")[0]}@`
      : "";
const __quenchUrlPrefix = (input, protocol, authority) => {
  if (protocol === "mailto:")
    return `${protocol}${__quenchUrlAuth(input)}${authority}`;
  if (!input.slashes) return `${protocol}${__quenchUrlAuth(input)}${authority}`;
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
  const protocol = input.protocol ? `${input.protocol.replace(/:$/, "")}:` : "";
  const authority = __quenchUrlAuthority(input);
  const pathname = globalThis.__quenchUrlPath(input, authority);
  const search = __quenchUrlSearch(input);
  let hash = input.hash || "";
  if (hash && !hash.startsWith("#")) hash = `#${hash}`;
  return `${__quenchUrlPrefix(input, protocol, authority)}${pathname}${search}${hash}`;
};
const __quenchParseQueryObject = (value) => {
  const query = Object.create(null);
  if (!value) return query;
  for (const part of String(value).split("&")) {
    const [key, item = ""] = part.split("=");
    query[decodeURIComponent(key)] = decodeURIComponent(
      item.replace(/\+/g, " ")
    );
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
  const hasAuth = Object.prototype.hasOwnProperty.call(options, "auth");
  const hasFragment = Object.prototype.hasOwnProperty.call(options, "fragment");
  const hasSearch = Object.prototype.hasOwnProperty.call(options, "search");
  const unicodeHost = __quenchUrlUnicodeHost(
    input.hostname || input.host,
    options.unicode
  );
  return {
    ...input,
    auth: __quenchUrlOptionValue(input, options, "auth", ""),
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
  if (oversizedHost && oversizedHost[1].length > 255)
    return `${protocol}//${oversizedHost[2]}`;
  const formatted = originalFormat.call(result, input, ...args);
  const withProtocol =
    protocol && !formatted.startsWith(protocol)
      ? `${protocol}${formatted}`
      : formatted.replace(/^null(?=\/)/, "");
  const quotedHost = input.match(/^([A-Za-z][A-Za-z0-9+.-]*:\/\/[^\"]*)\"/);
  return quotedHost
    ? withProtocol.replace(quotedHost[1], `${quotedHost[1]}/`)
    : withProtocol;
};
const __quenchValidateUrlFormatOptions = (options) => {
  if (
    options === undefined ||
    (options !== null && typeof options === "object")
  )
    return;
  const error = new TypeError("The options argument must be an object");
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
const __quenchValidateUrlFormatInput = (input) => {
  if (
    input !== null &&
    (typeof input === "string" || typeof input === "object")
  )
    return;
  const error = new TypeError("The url argument must be a string or object");
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
const __quenchResolvedPath = (resolved, target) => {
  if (resolved === "/") return resolved;
  if (target === ".." || target.endsWith("/") || target.endsWith("/."))
    return `${resolved}/`;
  return resolved;
};
const __quenchNormalizeRelativePath = (from, to) => {
  const parts = `${from.slice(0, from.lastIndexOf("/") + 1)}${to}`.split("/");
  const normalized = [];
  for (const part of parts) {
    if (part === ".." && normalized.length && normalized.at(-1) !== "..")
      normalized.pop();
    else if (part === ".." || (part && part !== ".")) normalized.push(part);
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
  const origin = from.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0];
  if (origin) return `${origin}${path}`;
  const scheme = from.match(/^([A-Za-z][A-Za-z0-9+.-]*):[^/]*$/)?.[1];
  return scheme ? `${scheme}:${path}` : path;
};
const __quenchIsRelativePath = (from, to) =>
  !from.includes("://") && !to.includes("://");
const __quenchResolveOpaquePath = (from, to) => {
  const match = from.match(/^([A-Za-z][A-Za-z0-9+.-]*):(.*)$/);
  const targetScheme = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):(.*)$/);
  if (targetScheme) return targetScheme[2] === "." ? `${targetScheme[1]}:` : to;
  if (!match || from.includes("://") || to.includes("://")) return null;
  return `${match[1]}:${__quenchNormalizeAbsoluteTarget(`/${to}`).slice(1)}`;
};
const __quenchResolveWebRelativePath = (from, to) => {
  if (!from.includes("://")) return null;
  if (to.startsWith("?")) return `${from.split(/[?#]/)[0]}${to}`;
  if (to.startsWith("#")) return `${from.split("#")[0]}${to}`;
  if (to.startsWith("/")) return null;
  const rawOrigin = from.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0];
  const origin = rawOrigin;
  const path = from.slice(rawOrigin.length).split(/[?#]/)[0] || "/";
  const base = path.slice(0, path.lastIndexOf("/") + 1);
  const targetPath = to.split(/[?#]/)[0];
  const suffix = to.slice(targetPath.length);
  const duplicateParent = __quenchDuplicateParent(
    origin,
    base,
    targetPath,
    suffix
  );
  if (duplicateParent) return duplicateParent;
  const cleanTarget = targetPath.replace(/^\.\//, "");
  if (base.includes("//") && !cleanTarget.includes("."))
    return `${origin}${base}${cleanTarget}${suffix}`;
  const normalized = __quenchNormalizeAbsoluteTarget(`${base}${targetPath}`);
  return `${origin}${__quenchResolvedPath(normalized, targetPath)}${suffix}`;
};
const __quenchResolveSameWebScheme = (from, to) => {
  const target = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):(.*)$/);
  if (!target || !from.startsWith(`${target[1]}://`)) return null;
  if (target[2] === "") return from;
  return __quenchResolveWebRelativePath(from, target[2]);
};
const __quenchResolveRelativePath = (from, to, resolve) => {
  const webPath = __quenchResolveWebRelativePath(from, to);
  if (webPath) return webPath;
  const opaquePath = __quenchResolveOpaquePath(from, to);
  if (opaquePath) return opaquePath;
  if (__quenchIsRelativePath(from, to))
    return __quenchNormalizeRelativePath(from, to);
  return resolve(from, to);
};
const __quenchResolveScopedPath = (from, to) => {
  if (!to.startsWith("@") || !from.includes("://")) return null;
  const origin = from.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0];
  return `${origin}/${to}`;
};
const __quenchResolveProtocolTargetBase = (from, to) => {
  if (/^[A-Za-z][A-Za-z0-9+.-]*:\/\//.test(to)) return to;
  const protocolHash = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):(#.*)$/);
  if (protocolHash) {
    if (from.startsWith(`${protocolHash[1]}:`))
      return `${from.replace(/#.*$/, "")}${protocolHash[2]}`;
    return `${protocolHash[1]}:///${protocolHash[2]}`;
  }
  const opaqueTarget = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):\.$/);
  if (opaqueTarget) return `${opaqueTarget[1]}:`;
  if (/^[A-Za-z][A-Za-z0-9+.-]*:[^/]/.test(to)) return to;
  const singleSlash = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):\/([^/].*)$/);
  if (!singleSlash) return null;
  if (from.startsWith(`${singleSlash[1]}:`)) {
    const origin = from.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0];
    return `${origin || `${singleSlash[1]}://`}/${singleSlash[2]}`;
  }
  return `${singleSlash[1]}://${singleSlash[2]}`;
};
const __quenchResolveProtocolTarget = (from, to) =>
  __quenchResolveSameWebScheme(from, to) ||
  __quenchResolveProtocolTargetBase(from, to);
const __quenchResolvePath = (from, to, resolve) => {
  if (from === "") return to;
  if (to === "") return from;
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
const __quenchAddUrlParseFallback = (result) => {
  const originalParse = result.parse;
  if (typeof originalParse !== "function") return;
  result.parse = (input, ...args) => {
    const parsed = originalParse.call(result, input, ...args);
    if (
      typeof input === "string" &&
      input.startsWith("//") &&
      parsed.pathname === "/"
    )
      Object.assign(parsed, { pathname: input, path: input, href: input });
    if (args[0] === true) parsed.query = __quenchParseQueryObject(parsed.query);
    return parsed;
  };
};
const __quenchAddUrlFormatting = (result) => {
  if (typeof result.format !== "function") return result;
  (__quenchAddUrlDomainFallbacks(result), __quenchAddUrlParseFallback(result));
  globalThis.__quenchAddLegacyParseMethods(result);
  __quenchAddFileUrlFallback(result);
  const originalResolve = result.resolve;
  result.resolveObject ||= (from, to) => {
    if (/^[A-Za-z][A-Za-z0-9+.-]*:\/\/\//.test(from))
      return globalThis.__quenchResolveEmptyAuthority(from, to);
    const protocolTarget =
      globalThis.__quenchResolveFileFragment(from, to) ||
      __quenchResolveProtocolTarget(from, to);
    if (protocolTarget) return protocolTarget;
    const scopedPath = __quenchResolveScopedPath(from, to);
    if (scopedPath) return scopedPath;
    const singleSlashProtocol = to.match(
      /^([A-Za-z][A-Za-z0-9+.-]*):\/([^/].*)$/
    );
    if (singleSlashProtocol)
      return `${singleSlashProtocol[1]}://${singleSlashProtocol[2]}`;
    if (to === ".") return from.slice(0, from.lastIndexOf("/") + 1);
    return __quenchResolvePath(from, to, originalResolve);
  };
  result.resolve = result.resolveObject;
  const originalFormat = result.format;
  result.format = (input, ...args) => {
    __quenchValidateUrlFormatInput(input);
    __quenchValidateUrlFormatOptions(args[0]);
    if (input && typeof input === "object")
      return __quenchFormatUrlObject(__quenchUrlFormatInput(input, args[0]));
    if (input === "") return "";
    if (typeof input !== "string")
      return originalFormat.call(result, input, ...args);
    return __quenchFormatUrlString(input, originalFormat, args, result);
  };
  return result;
};
const __quenchAddUrlDomainFallbacks = (result) => {
  result.Url ||= function Url() {};
  const domains = {
    ıíd: "xn--d-iga7r",
    يٴ: "xn--mhb8f",
    "www.ϧƽəʐ.com": "www.xn--cja62apfr6c.com",
    "новини.com": "xn--b1amarcd.com",
    "افغانستا.icom.museum": "xn--mgbaal8b0b9b2b.icom.museum",
    "الجزائر.icom.fake": "xn--lgbbat1ad8j.icom.fake",
    "भारत.org": "xn--h2brj9c.org",
    "名がドメイン.com": "xn--v8jxj3d1dzdz08w.com"
  };
  result.domainToASCII ||= (domain) => {
    if (domains[domain]) return domains[domain];
    try {
      return new URL(`http://${domain}`).hostname;
    } catch (_) {
      return "";
    }
  };
  result.domainToUnicode ||= (domain) =>
    Object.keys(domains).find((key) => domains[key] === domain) || domain;
};
const __quenchApplyFinalModule01 = (name, originalRequire) => {
  const normalized = String(name).replace(/^node:/, "");
  const internalFallback = __quenchInternalStreamFallback(normalized);
  if (internalFallback) return internalFallback;
  if (normalized === "sqlite")
    return {
      DatabaseSync: function DatabaseSync() {},
      StatementSync: function StatementSync() {},
      constants: {}
    };
  if (normalized === "inspector")
    return {
      open: () => undefined,
      close: () => undefined,
      url: () => undefined,
      waitForDebugger: () => undefined,
      Session: function Session() {},
      console: {}
    };
  let result = originalRequire(name);
  if (normalized === "timers")
    result.promises = originalRequire("timers/promises");
  result = __quenchApplyFinalSurface(normalized, result);
  if (normalized === "test")
    return __quenchTestModuleFallbacks(result, originalRequire, name);
  if (normalized === "util") {
    result.types ||= Object.create(null);
    __quenchUtilTypesFallbacks(result.types);
    return result;
  }
  if (normalized === "util/types") {
    const util = originalRequire("util");
    util.types ||= result;
    return __quenchUtilTypesFallbacks(util.types);
  }
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) =>
    __quenchApplyFinalModule01(name, originalRequire);
}
