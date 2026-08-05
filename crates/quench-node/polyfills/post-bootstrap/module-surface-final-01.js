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
  __quenchUtilTypesBasicFallbacks(result);
  __quenchUtilTypesCollectionFallbacks(result);
  __quenchUtilTypesTypedFallbacks(result);
  return result;
};
const __quenchInternalStreamFallback = (normalized) => {
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
  let authority = input.host || input.hostname || "";
  if (authority.includes(":") && !authority.startsWith("[") && input.hostname)
    authority = `[${authority}]`;
  if (input.port && !authority.endsWith(`:${input.port}`))
    authority = `${authority}:${input.port}`;
  return authority;
};
const __quenchUrlAuth = (input) =>
  input.auth ? `${encodeURIComponent(input.auth).replace(/%3A/gi, ":")}@` : "";
const __quenchUrlPrefix = (input, protocol, authority) => {
  const slashedProtocol = ["http:", "https:", "ftp:", "gopher:", "file:"];
  if (!input.slashes && !slashedProtocol.includes(protocol))
    return `${protocol}${__quenchUrlAuth(input)}${authority}`;
  return `${protocol}//${__quenchUrlAuth(input)}${authority}`;
};
const __quenchUrlPath = (input, authority) => {
  let pathname = input.pathname || "";
  pathname = pathname.replace(/#/g, "%23").replace(/\?/g, "%3F");
  if (authority && pathname && !pathname.startsWith("/"))
    pathname = `/${pathname}`;
  if (
    authority &&
    !pathname &&
    (input.search !== undefined || input.query !== undefined)
  )
    pathname = "/";
  return pathname;
};
const __quenchUrlSearch = (input) => {
  let search = input.search || "";
  if (!search && input.query !== undefined) {
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
  const pathname = __quenchUrlPath(input, authority);
  const search = __quenchUrlSearch(input);
  let hash = input.hash || "";
  if (hash && !hash.startsWith("#")) hash = `#${hash}`;
  return `${__quenchUrlPrefix(input, protocol, authority)}${pathname}${search}${hash}`;
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
const __quenchAddUrlFormatting = (result) => {
  if (typeof result.format !== "function") return result;
  const originalFormat = result.format;
  result.format = (input, ...args) => {
    if (input && typeof input === "object")
      return __quenchFormatUrlObject(input);
    if (typeof input !== "string")
      return originalFormat.call(result, input, ...args);
    return __quenchFormatUrlString(input, originalFormat, args, result);
  };
  return result;
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
