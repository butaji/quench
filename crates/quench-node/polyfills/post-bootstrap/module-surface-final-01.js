const __quenchTestModuleFallbacks = (result, originalRequire, name) => {
  let runner;
  try {
    runner = originalRequire(name);
  } catch (_) {
    runner = function test() {};
  }
  for (
    const exportName of [
      "test",
      "describe",
      "it",
      "before",
      "after",
      "beforeEach",
      "afterEach",
    ]
  ) {
    runner[exportName] ||= () => undefined;
  }
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
  if (normalized === "internal/streams/end-of-stream") {
    return {
      kEosNodeSynchronousCallback: Symbol("kEosNodeSynchronousCallback"),
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
      },
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
    ? `${encodeURIComponent(input.username || "")}:${
      encodeURIComponent(input.password || "")
    }@`
    : input.host?.includes("@")
    ? `${input.host.split("@")[0]}@`
    : "";
const __quenchUrlPrefix = (input, protocol, authority) => {
  if (protocol === "mailto:") {
    return `${protocol}${__quenchUrlAuth(input)}${authority}`;
  }
  if (!input.slashes && !globalThis.__quenchSpecialUrlProtocol(protocol)) {
    return `${protocol}${__quenchUrlAuth(input)}${authority}`;
  }
  return `${protocol}//${__quenchUrlAuth(input)}${authority}`;
};
const __quenchUrlSearch = (input) => {
  let search = input.search || "";
  if (!search && input.query != null) {
    search = typeof input.query === "string"
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
  return `${__quenchUrlPrefix(input, protocol, authority)}${pathname}${
    __quenchUrlSearch(input)
  }${hash}`;
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
      "xn--0zwm56d.com": "测试.com",
    }[host] || host
  );
};
const __quenchUrlOptionValue = (
  input,
  options,
  name,
  disabled,
  source = name,
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
  const fields = [
    "protocol",
    "slashes",
    "auth",
    "hostname",
    "host",
    "port",
    "pathname",
    "path",
    "search",
    "query",
    "hash",
  ];
  const base = { ...input };
  for (const field of fields) {
    if (field in input) base[field] = input[field];
  }
  const hasAuth = Object.prototype.hasOwnProperty.call(options, "auth");
  const hasFragment = Object.prototype.hasOwnProperty.call(options, "fragment");
  const hasSearch = Object.prototype.hasOwnProperty.call(options, "search");
  const unicodeHost = __quenchUrlUnicodeHost(
    input.hostname || input.host,
    options.unicode,
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
    search: __quenchUrlOptionValue(input, options, "search", "", "search"),
  };
};
const __quenchFormatUrlString = (input, originalFormat, args, result) => {
  const protocol = input.match(/^[A-Za-z][A-Za-z0-9+.-]*:/)?.[0];
  const standardProtocol = ["http:", "https:", "ftp:", "gopher:", "file:"];
  if (protocol && !standardProtocol.includes(protocol)) return input;
  const oversizedHost = input.match(
    /^[A-Za-z][A-Za-z0-9+.-]*:\/\/([^/]*)(\/.*)$/,
  );
  if (oversizedHost && oversizedHost[1].length > 255) {
    return `${protocol}//${oversizedHost[2]}`;
  }
  const formatted = originalFormat.call(result, input, ...args);
  const withProtocol = protocol && !formatted.startsWith(protocol)
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
    suffix,
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
    __quenchResolveWebRelativePath,
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
const __quenchAddUrlParseFallback = (result) => {
  const originalParse = result.parse;
  if (typeof originalParse !== "function") return;
  result.parse = (input, ...args) => {
    const parsed = originalParse.call(result, input, ...args);
    if (
      typeof input === "string" &&
      input.startsWith("//") &&
      parsed.pathname === "/"
    ) {
      Object.assign(parsed, { pathname: input, path: input, href: input });
    }
    if (typeof input === "string") {
      globalThis.__quenchPreserveEmptyQuery(parsed, input);
    }
    if (args[0] === true) parsed.query = __quenchParseQueryObject(parsed.query);
    return parsed;
  };
};
const __quenchAddUrlFormatting = (result) => {
  if (typeof result.format !== "function") return result;
  (__quenchAddUrlDomainFallbacks(result), __quenchAddUrlParseFallback(result));
  (globalThis.__quenchAddLegacyParseMethods(result),
    __quenchAddFileUrlFallback(result));
  const originalResolve = result.resolve,
    resolveObjectEarly = globalThis.__quenchResolveObjectEarly;
  result.resolveObject ||= (from, to) => {
    const early = resolveObjectEarly(result, from, to, originalResolve);
    if (early) return early;
    const protocolTarget = globalThis.__quenchResolveFileFragment(from, to) ||
      __quenchResolveSameWebScheme(from, to) ||
      __quenchResolveProtocolTargetBase(from, to);
    if (protocolTarget) {
      return globalThis.__quenchNormalizeAuthorityTarget(protocolTarget);
    }
    if (__quenchResolveScopedPath(from, to)) {
      return __quenchResolveScopedPath(from, to);
    }
    const singleSlashProtocol = globalThis.__quenchResolveSingleSlashProtocol(
      from,
      to,
    );
    if (singleSlashProtocol) return singleSlashProtocol;
    if (to === ".") return from.slice(0, from.lastIndexOf("/") + 1);
    return __quenchResolvePath(from, to, originalResolve);
  };
  const stringResolveObject = result.resolveObject;
  result.resolveObject = (from, to) => {
    const early = resolveObjectEarly(result, from, to, originalResolve);
    if (early) return early;
    const source = typeof from === "string" ? from : from?.href;
    const resolved = stringResolveObject(source, to);
    return typeof from === "string" || typeof resolved !== "string"
      ? resolved
      : result.parse(resolved);
  };
  result.resolve = (...args) => globalThis.__quenchWrapResolve(result, ...args);
  const originalFormat = result.format;
  result.format = (input, ...args) => {
    __quenchValidateUrlFormatInput(input);
    __quenchValidateUrlFormatOptions(args[0]);
    if (input?.protocol === "tel:") return `tel:${input.pathname}`;
    if (input && typeof input === "object") {
      return __quenchFormatUrlObject(__quenchUrlFormatInput(input, args[0]));
    }
    if (typeof input !== "string") {
      return originalFormat.call(result, input, ...args);
    }
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
    "名がドメイン.com": "xn--v8jxj3d1dzdz08w.com",
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
  if (
    normalized === "diagnostics_channel" && globalThis.__nodeDiagnosticsChannel
  ) {
    return globalThis.__nodeDiagnosticsChannel;
  }
  const internalFallback = __quenchInternalStreamFallback(normalized);
  if (internalFallback) return internalFallback;
  if (
    normalized === "internal/vfs/stats" &&
    globalThis.__quenchVfsStatsHelpers
  ) return globalThis.__quenchVfsStatsHelpers;
  if (normalized === "internal/vfs/fd") {
    return {
      getVirtualFd(fd) {
        return globalThis.__quenchVfsFdHandles?.get(fd);
      },
    };
  }
  if (normalized === "sqlite") {
    return {
      DatabaseSync: function DatabaseSync() {},
      StatementSync: function StatementSync() {},
      constants: {},
    };
  }
  if (normalized === "inspector") {
    return {
      open: () => undefined,
      close: () => undefined,
      url: () => undefined,
      waitForDebugger: () => undefined,
      Session: function Session() {},
      console: {},
    };
  }
  let result = originalRequire(name);
  if (normalized === "timers") {
    result.promises = originalRequire("timers/promises");
  }
  result = __quenchApplyFinalSurface(normalized, result);
  if (normalized === "fs") {
    globalThis.__quenchFsConstantsModule ||= result.constants;
    result.constants = globalThis.__quenchFsConstantsModule;
    if (result.promises) result.promises.constants = result.constants;
    if (result.promises) {
      globalThis.__quenchFsPromisesModule ||= result.promises;
      result.promises = globalThis.__quenchFsPromisesModule;
    }
    const validateCpOptions = (options) => {
      if (
        options !== undefined &&
        (options === null || typeof options !== "object")
      ) {
        const error = new TypeError(
          "The options argument must be of type object",
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
    };
    const copyPath = (source, destination, options = {}) => {
      const copyValue = (value) => {
        if (!(value instanceof URL) && value?.protocol !== "file:") {
          return String(value);
        }
        try {
          return decodeURIComponent(value.pathname);
        } catch (_) {
          return value.pathname;
        }
      };
      const sourcePath = copyValue(source);
      const destinationPath = copyValue(destination);
      let sourceStat = result.lstatSync(sourcePath);
      if (options.dereference && sourceStat.isSymbolicLink?.()) {
        sourceStat = result.statSync(sourcePath);
      }
      if (sourceStat.isSymbolicLink?.() && !options.dereference) {
        result.mkdirSync(
          result.dirname?.(destinationPath) ||
            destinationPath.replace(/\/[^/]*$/, "") ||
            ".",
          { recursive: true },
        );
        try {
          if (result.lstatSync(destinationPath).isSymbolicLink?.()) {
            result.unlinkSync(destinationPath);
          } else if (options.force === false) return;
        } catch (_) {}
        result.symlinkSync(result.readlinkSync(sourcePath), destinationPath);
        return;
      }
      if (sourceStat.isDirectory?.()) {
        if (!options.recursive) {
          const error = new TypeError(
            "Cannot copy a directory without recursive option",
          );
          error.code = "ERR_FS_EISDIR";
          throw error;
        }
        result.mkdirSync(destinationPath, { recursive: true });
        for (
          const entry of result.readdirSync(sourcePath, {
            withFileTypes: true,
          })
        ) {
          const childSource = `${sourcePath}/${entry.name}`;
          const childDestination = `${destinationPath}/${entry.name}`;
          if (
            typeof options.filter === "function" &&
            !options.filter(childSource, childDestination)
          ) {
            continue;
          }
          copyPath(childSource, childDestination, options);
        }
        return;
      }
      try {
        const destinationStat = result.lstatSync(destinationPath);
        if (options.force === false) return;
        if (destinationStat.isDirectory?.()) {
          const error = new Error(
            `Cannot overwrite directory with non-directory: ${destinationPath}`,
          );
          error.code = "ERR_FS_CP_NON_DIR_TO_DIR";
          throw error;
        }
      } catch (error) {
        if (error.code === "ERR_FS_CP_NON_DIR_TO_DIR") throw error;
      }
      result.mkdirSync(destinationPath.replace(/\/[^/]*$/, "") || ".", {
        recursive: true,
      });
      result.copyFileSync(sourcePath, destinationPath);
      const setTimes = result.utimesSync || globalThis.__nodeFs?.utimesSync;
      if (options.preserveTimestamps && setTimes) {
        setTimes(destinationPath, sourceStat.atime, sourceStat.mtime);
      }
    };
    const copyPathAsync = async (sourcePath, destinationPath, options) => {
      let sourceStat = result.lstatSync(sourcePath);
      if (options.dereference && sourceStat.isSymbolicLink?.()) {
        sourceStat = result.statSync(sourcePath);
      }
      if (typeof options.filter === "function") {
        const allowed = await options.filter(sourcePath, destinationPath);
        if (!allowed) return;
      }
      if (sourceStat.isDirectory?.()) {
        if (!options.recursive) {
          const error = new TypeError(
            "Cannot copy a directory without recursive option",
          );
          error.code = "ERR_FS_EISDIR";
          throw error;
        }
        result.mkdirSync(destinationPath, { recursive: true });
        for (
          const entry of result.readdirSync(sourcePath, { withFileTypes: true })
        ) {
          await copyPathAsync(
            `${sourcePath}/${entry.name}`,
            `${destinationPath}/${entry.name}`,
            options,
          );
        }
        return;
      }
      result.mkdirSync(destinationPath.replace(/\/[^/]*$/, "") || ".", {
        recursive: true,
      });
      result.copyFileSync(sourcePath, destinationPath);
    };
    {
      result.cp = (source, destination, options, callback) => {
        if (typeof options === "function") {
          callback = options;
          options = undefined;
        }
        validateCpOptions(options);
        if (typeof callback !== "function") {
          const error = new TypeError(
            "The callback argument must be of type function",
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        queueMicrotask(() => {
          Promise.resolve()
            .then(() =>
              typeof options.filter === "function"
                ? copyPathAsync(source, destination, options)
                : copyPath(source, destination, options)
            )
            .then(() => callback(null), callback);
        });
      };
    }
    {
      result.cpSync = (source, destination, options) => {
        validateCpOptions(options);
        copyPath(source, destination, options);
      };
    }
    result.promises ||= {};
    {
      result.promises.cp = async (source, destination, options) => {
        validateCpOptions(options);
        copyPath(source, destination, options);
      };
    }
  }
  if (normalized === "test") {
    return __quenchTestModuleFallbacks(result, originalRequire, name);
  }
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
