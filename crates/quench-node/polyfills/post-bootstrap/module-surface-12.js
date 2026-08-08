const __quenchSetFallbacks = (result, names, fallback) => {
  for (const name of names) result[name] ||= fallback;
};
const __quenchWorkerThreadFallbacks = (result) => {
  __quenchSetFallbacks(
    result,
    [
      "Worker",
      "MessageChannel",
      "MessagePort",
      "BroadcastChannel",
      "receiveMessageOnPort",
      "markAsUncloneable",
      "setEnvironmentData",
      "getEnvironmentData",
      "markAsUntransferable",
      "isMarkedAsUncloneable",
      "moveMessagePortToContext"
    ],
    () => undefined
  );
  if (globalThis.MessageChannel) {
    result.MessageChannel = globalThis.MessageChannel;
  }
  if (globalThis.MessagePort) result.MessagePort = globalThis.MessagePort;
  result.parentPort ??= null;
  result.workerData ??= undefined;
  result.threadId ??= 0;
};
const __quenchFsModuleFallbacks = (result) => {
  __quenchSetFallbacks(
    result,
    ["glob", "watch", "watchFile", "unwatchFile"],
    () => undefined
  );
  __quenchSetFallbacks(
    result,
    [
      "FSWatcher",
      "StatWatcher",
      "opendir",
      "opendirSync",
      "Dir",
      "Dirent",
      "ReadStream",
      "WriteStream"
    ],
    function Constructor() {}
  );
  result.promises ||= {};
  result.promises.glob ||= async function* () {};
  result.promises.cp ||= async () => undefined;
  result.promises.opendir ||= async () => undefined;
};
const __quenchZlibFallbacks = (result) =>
  __quenchSetFallbacks(
    result,
    [
      "deflateRaw",
      "deflateRawSync",
      "inflateRaw",
      "inflateRawSync",
      "brotliCompress",
      "brotliCompressSync",
      "brotliDecompress",
      "brotliDecompressSync",
      "unzip",
      "unzipSync"
    ],
    () => undefined
  );
const __quenchApplyModuleSurface12 = (name, result) => {
  const normalized = String(name).replace(/^node:/, "");
  if (normalized === "worker_threads") __quenchWorkerThreadFallbacks(result);
  if (normalized === "fs") __quenchFsModuleFallbacks(result);
  if (normalized === "zlib") __quenchZlibFallbacks(result);
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) =>
    __quenchApplyModuleSurface12(name, originalRequire(name));
}
globalThis.__nodeLegacyPathEncode = (value) =>
  encodeURIComponent(value)
    .replace(/%2F/gi, "/")
    .replace(/%3A/gi, ":")
    .replace(/%40/gi, "@")
    .replace(/%5B/gi, "[")
    .replace(/%5D/gi, "]");
globalThis.__nodeLegacyPathNormalize = (value) =>
  value
    .replaceAll("\\", "/")
    .replaceAll('"', "%22")
    .replaceAll("'", "%27")
    .replaceAll("<", "%3C")
    .replaceAll(">", "%3E")
    .replaceAll("`", "%60")
    .replaceAll("{", "%7B")
    .replaceAll("}", "%7D")
    .replaceAll("|", "%7C")
    .replaceAll("^", "%5E")
    .replaceAll(" ", "%20");
globalThis.__nodeLegacyUrlControlNormalize = (value) =>
  value.replace(/\t/g, "%09").replace(/\n/g, "%0A").replace(/\r/g, "%0D");
globalThis.__nodeLegacyQueryNormalize = (value) =>
  value
    .replaceAll("\\", "%5C")
    .replaceAll('"', "%22")
    .replaceAll("'", "%27")
    .replaceAll("<", "%3C")
    .replaceAll(">", "%3E")
    .replaceAll("`", "%60")
    .replaceAll("{", "%7B")
    .replaceAll("}", "%7D")
    .replaceAll("|", "%7C")
    .replaceAll("^", "%5E")
    .replaceAll(" ", "%20");
globalThis.__nodeLegacyUrlHrefPath = (pathname) =>
  pathname.startsWith(";") ? `/${pathname}` : pathname;
globalThis.__nodeLegacyUrlHostValue = (protocol, host) =>
  protocol === "file:" && !host ? "" : host || null;
globalThis.__nodeLegacyUrlHrefPrefix = (protocol, auth, host) =>
  protocol === "file:"
    ? `${protocol}//${host ? `${auth}${host}` : ""}`
    : `${protocol}${host ? `//${auth}${host}` : ""}`;
globalThis.__nodeLegacyUrlSlashes = (input, protocol) =>
  input.startsWith("//") ||
  (Boolean(protocol) && input.slice(protocol.length).startsWith("//"));
globalThis.__quenchUrlPath = (input, authority) => {
  let pathname = (input.pathname || "")
    .replace(/#/g, "%23")
    .replace(/\?/g, "%3F")
    .replace(/ /g, "%20");
  if (authority && pathname && !pathname.startsWith("/")) {
    pathname = `/${pathname}`;
  }
  if (
    authority &&
    input.protocol !== "mailto:" &&
    !pathname &&
    (input.search != null || input.query != null)
  ) {
    pathname = "/";
  }
  return pathname;
};
globalThis.__quenchResolveEmptyAuthority = (from, to) => {
  if (/^[a-z][a-z0-9+.-]*:/i.test(to)) return to;
  if (to.startsWith("/")) {
    return globalThis.__quenchResolveEmptyNetworkPath(from, to);
  }
  let base = from.replace(/[^/]*$/, "");
  let target = to;
  let parentTraversal = 0;
  while (target.startsWith("../")) {
    parentTraversal += 1;
    base = base.replace(/[^/]+\/$/, "");
    target = target.slice(3);
  }
  if (parentTraversal > 3) base = base.replace(/[^/]+\/$/, "");
  if (parentTraversal > 2) {
    base = base.replace(
      /^(.*?:\/\/)(.*)$/,
      (_, prefix, path) => prefix + path.replace(/\/{2,}/g, "/")
    );
  }
  if (parentTraversal > 3) base = base.replace(/[^/]+\/$/, "");
  return base.replace(/^http:\/\/\//, "http:/") + target.replace(/^\.\//, "");
};
globalThis.__quenchResolveEmptyNetworkPath = (from, to) =>
  `${from.match(/^[a-z][a-z0-9+.-]*:\/\//i)?.[0] || ""}${
    to.startsWith("//") ? to.slice(2) : to
  }${/^\/\/[^/]+$/.test(to) && /^https?:/i.test(from) ? "/" : ""}`.replace(
    /^http:\/\/\//,
    "http:/"
  );
globalThis.__quenchResolveFileFragment = (from, to) =>
  /^file:\/[^/]/i.test(from) && /^#/.test(to)
    ? from.replace(/^file:\//i, "file:///") + to
    : /^file:\/[^/]/i.test(to) && /^#/.test(from)
      ? to.replace(/^file:\//i, "file:///") + from
      : null;
globalThis.__quenchResolveReversedFileRelative = (from, to) =>
  /^file:\/[^/]/i.test(to) && !/^[a-z][a-z0-9+.-]*:/i.test(from)
    ? globalThis.__quenchResolveFileRelative(to, from)
    : null;
globalThis.__quenchResolveFileAuthority = (from, to) =>
  /^file:\/\/[^/]+\//i.test(from) && /^file:\/[^/]/i.test(to)
    ? from.replace(/^file:\/\/[^/]+/i, "file://")
    : /^file:\/[^/]/i.test(from) && /^file:\/\/[^/]+\//i.test(to)
      ? to.replace(/^file:\/\/[^/]+/i, "file://")
      : null;
const __quenchResolveFileRelativeBase = (from, to) => {
  const reversed = globalThis.__quenchResolveReversedFileRelative(from, to);
  if (reversed) return reversed;
  if (/^file:\/[^/]/i.test(to) && from.startsWith("/")) {
    return to.replace(/^file:\//i, "file:///") + from;
  }
  if (
    !/^file:\/[^/]/i.test(from) ||
    /^[a-z][a-z0-9+.-]*:/i.test(to) ||
    to.startsWith("#")
  ) {
    return null;
  }
  if (to === "") return from.replace(/^file:\//i, "file:///");
  if (to.startsWith("/")) return `file://${to}`;
  let base = from.replace(/^file:\//i, "file:///").replace(/[^/]*$/, "");
  let target = to;
  while (target.startsWith("../")) {
    base = base.replace(/[^/]+\/$/, "");
    target = target.slice(3);
  }
  return base + target.replace(/^\.\//, "").replace(/#$/, "");
};
globalThis.__quenchResolveFileRelative = (from, to) =>
  globalThis.__quenchResolveQueryAbsolute(from, to) ||
  globalThis.__quenchResolveMailtoRelative(from, to) ||
  globalThis.__quenchResolveFileAuthority(from, to) ||
  __quenchResolveFileRelativeBase(from, to);
globalThis.__nodeLegacyResolve = (from, to) =>
  globalThis.__quenchResolveFileRelative(from, to) ||
  new globalThis.__nodeURL(to, from).href;
globalThis.__quenchResolveQueryAbsolute = (from, to) =>
  typeof from === "string" &&
  from.startsWith("/") &&
  /^https?:\/\/[^/?#]+(?:[?#].*)?$/i.test(to)
    ? to.split(/[?#]/)[0] + from
    : null;
globalThis.__quenchResolveMailtoRelative = (from, to) =>
  /^mailto:/i.test(from) && /^[?#]/.test(to)
    ? `mailto:${from.slice(7).split(/[?#]/)[0]}${to}`
    : /^mailto:/i.test(from) && !/^[a-z][a-z0-9+.-]*:/i.test(to)
      ? `mailto:${from.slice(7, from.lastIndexOf("/") + 1)}${to}`
      : /^mailto:/i.test(to) && !/^[a-z][a-z0-9+.-]*:/i.test(from)
        ? `mailto:${to.slice(7, to.lastIndexOf("/") + 1)}${from}`
        : null;
globalThis.__quenchOpaqueTargetRelative = (from, to) => {
  const target = to.match(/^([a-z][a-z0-9+.-]*):(.*)$/i);
  if (!target || /^[a-z][a-z0-9+.-]*:/i.test(from)) return null;
  if (from === "") return `${target[1]}:${target[2].replace(/#.*$/, "")}`;
  if (from.startsWith("/")) return `${target[1]}:${from}`;
  return globalThis.__quenchNormalizeOpaqueRelative(
    target[1],
    target[2].slice(0, target[2].lastIndexOf("/") + 1),
    from
  );
};
globalThis.__quenchOpaqueTargetResolve = (from, to, target) =>
  target[2] === "."
    ? `${target[1]}:`
    : globalThis.__quenchOpaqueTargetRelative(from, to) || to;
globalThis.__quenchResolveSingleSlashProtocol = (from, to) => {
  const match = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):\/([^/].*)$/);
  if (!match) return null;
  const origin = from.startsWith(`${match[1]}:`)
    ? from.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0]
    : null;
  return origin ? `${origin}/${match[2]}` : `${match[1]}://${match[2]}`;
};
globalThis.__quenchNormalizeOpaqueRelative = (scheme, base, target) => {
  const parts = `${base}${target}`.split("/");
  const normalized = [];
  for (const part of parts) {
    if (part === "..") normalized.pop();
    else if (part && part !== ".") normalized.push(part);
  }
  return `${scheme}:${base.startsWith("/") ? "/" : ""}${normalized.join("/")}`;
};
globalThis.__quenchIsOpaqueTarget = (value) =>
  /^[A-Za-z][A-Za-z0-9+.-]*:[^/]/.test(value) && !/^file:/i.test(value);
globalThis.__quenchResolveEmptyBase = (to) =>
  globalThis.__quenchIsOpaqueTarget(to) ? to.replace(/#.*$/, "") : to;
globalThis.__quenchResolveEmptyTarget = (from) =>
  globalThis.__quenchIsOpaqueTarget(from) ? from.replace(/#.*$/, "") : from;
globalThis.__quenchPreserveWebDoubleSlash = (base, target) =>
  target.includes("//") || (base.includes("//") && !target.includes("."));
globalThis.__quenchNormalizeAuthorityTarget = (value) => {
  const normalized = value.replace(
    /^([A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]+)\/{2,}$/,
    "$1/"
  );
  return /^(https?):\/\/[^/?#]+$/i.test(normalized)
    ? `${normalized}/`
    : normalized;
};
globalThis.__quenchIsAuthorityOnly = (value) =>
  /^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/?#]+\/?$/.test(value);
globalThis.__quenchResolveAuthorityRelative = (from, target) => {
  const path = from.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*(\/[^?#]*)/)?.[1];
  return `${target}${path || "/"}`;
};
globalThis.__quenchResolveSameWebAuthority = (from, to, target) =>
  globalThis.__quenchIsAuthorityOnly(to)
    ? globalThis.__quenchResolveAuthorityRelative(
        from,
        `${target[1]}:${target[2]}`
      )
    : target[2].startsWith("//")
      ? `${target[1]}:${target[2]}`
      : null;
globalThis.__quenchResolveSameWebScheme = (from, to, relative) => {
  const target = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):(.*)$/);
  if (!target || !from.startsWith(`${target[1]}://`)) return null;
  if (target[2] === "") return from;
  return (
    globalThis.__quenchResolveSameWebAuthority(from, to, target) ||
    relative(from, target[2])
  );
};
globalThis.__quenchResolveAuthenticatedTarget = (from, to) => {
  if (typeof from !== "string" || !from.includes("@")) return null;
  const origin = from.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0];
  const path = to.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*(\/[^?#]*)/)?.[1];
  return origin && path ? `${origin}${path}` : null;
};
globalThis.__quenchResolveAbsoluteTarget = (from, to) => {
  if (
    !/^[A-Za-z][A-Za-z0-9+.-]*:\/\//.test(to) ||
    /^file:/i.test(to) ||
    globalThis.__quenchIsAuthorityOnly(to)
  ) {
    return null;
  }
  return (
    globalThis.__quenchResolveAuthenticatedTarget(from, to) ||
    globalThis.__quenchNormalizeAuthorityTarget(to)
  );
};
globalThis.__quenchResolveFragmentOnly = (from, to) =>
  typeof from === "string" && from.startsWith("#") && to.startsWith("#")
    ? `/${to}`
    : null;
globalThis.__quenchWrapResolve = (result, from, to) => {
  if (/^[a-z][a-z0-9+.-]*:\/\/\//i.test(from) && /^[^/]/.test(to)) {
    if (/^(?:\.\.\/)+/.test(to)) {
      const prefix = from.slice(0, from.indexOf(":") + 4);
      const path = from.slice(from.indexOf("://") + 3).split(/[?#]/)[0];
      const parts = `${path.slice(0, path.lastIndexOf("/") + 1)}${to}`.split(
        "/"
      );
      const normalized = [];
      for (const part of parts) {
        if (!part || part === ".") continue;
        if (part === "..") {
          if (normalized.length > 1) normalized.pop();
        } else normalized.push(part);
      }
      return `${prefix}${normalized.join("/")}`;
    }
    const prefix = from.slice(0, from.indexOf(":") + 3);
    const path = from.slice(from.indexOf("://") + 3);
    const base = path.slice(0, path.lastIndexOf("/") + 1);
    return `${prefix}/${base.replace(/^\/+/, "")}${to}`;
  }
  if (
    typeof from === "string" &&
    from.startsWith("/") &&
    /^https?:\/\/[^/?#]+(?:\?.*)?$/i.test(to)
  ) {
    const authority = to.match(/^[^?]+/)[0];
    const query = to.slice(authority.length).replace(/^\?/, "");
    const path = from.split("?")[0];
    const fromQuery = from.includes("?")
      ? from.slice(from.indexOf("?") + 1)
      : "";
    return `${authority}${path}${
      fromQuery ? `?${fromQuery}` : query ? `?${query}` : ""
    }`;
  }
  const resolved = result.resolveObject(from, to);
  return globalThis.__quenchNormalizeAuthorityTarget(
    typeof resolved === "string" ? resolved : resolved.href
  );
};
globalThis.__quenchResolveLegacyFileRelative = (from, to) =>
  globalThis.__quenchIsOpaqueTarget(from) ||
  globalThis.__quenchIsOpaqueTarget(to) ||
  (/^[A-Za-z][A-Za-z0-9+.-]*:\/\//.test(to) && !/^file:/i.test(to)) ||
  (typeof from === "string" && from.includes("://") && to.startsWith("//"))
    ? null
    : globalThis.__quenchResolveFileRelative(from, to);
globalThis.__quenchResolveParsedAbsolute = (result, from, to) => {
  if (typeof from === "string" || !to.startsWith("/")) return null;
  const source = from.href || result.format(from);
  const resolved = globalThis.__quenchResolveAbsolutePath(source, to);
  if (!resolved) return null;
  const parsed = result.parse(resolved);
  const resolvedPath = resolved.includes("://") ? parsed.pathname : resolved;
  return Object.assign(parsed, {
    protocol: from.protocol,
    pathname: resolvedPath,
    path: resolvedPath,
    href: resolved
  });
};
globalThis.__quenchResolveScopedObject = (r, f, t) =>
  typeof f === "string" || !t.startsWith("@") || !f.href
    ? null
    : r.parse(
        `${f.href.match(/^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/]*/)?.[0]}/${t}`
      );
globalThis.__quenchResolveParsedSpecial = (r, f, t) =>
  globalThis.__quenchResolveScopedObject(r, f, t) ||
  globalThis.__quenchResolveParsedFile(r, f, t) ||
  globalThis.__quenchResolveParsedTextSpecial(r, f, t) ||
  globalThis.__quenchResolveParsedFragment(r, f, t) ||
  globalThis.__quenchResolveParsedWebAbsolute(r, f, t) ||
  globalThis.__quenchResolveParsedSameSchemeOpaque(r, f, t) ||
  globalThis.__quenchResolveParsedAbsoluteOpaque(r, f, t) ||
  globalThis.__quenchResolveParsedEmptyScheme(r, f, t) ||
  globalThis.__quenchResolveParsedWebRelative(r, f, t) ||
  globalThis.__quenchResolveParsedOpaque(r, f, t);
globalThis.__quenchResolveParsedObject = (result, from, to) => {
  if (typeof from === "string") return null;
  const protocolHash = to.match(/^([A-Za-z][A-Za-z0-9+.-]*):(#.*)$/);
  if (protocolHash && from.protocol === `${protocolHash[1]}:`) {
    return result.parse(`${from.href.split("#")[0]}${protocolHash[2]}`);
  }
  if (protocolHash) {
    return result.parse(`${protocolHash[1]}:///${protocolHash[2]}`);
  }
  const singleSlash = globalThis.__quenchResolveSingleSlashProtocol(
    from.href || from.pathname || "",
    to
  );
  if (singleSlash) return result.parse(singleSlash);
  const absoluteTarget = globalThis.__quenchResolveParsedAbsoluteTarget(
    result,
    from,
    to
  );
  if (absoluteTarget) return absoluteTarget;
  const fragment = globalThis.__quenchResolveParsedSpecial(result, from, to);
  if (fragment) return fragment;
  return result.parse(
    globalThis.__quenchResolveParsedPath(from.pathname || from.href || "", to)
  );
};
globalThis.__quenchResolveParsedPath = (base, to) => {
  const path = to.startsWith("/")
    ? to
    : `${base.slice(0, base.lastIndexOf("/") + 1)}${to}`;
  const parts = path.split("/");
  const normalized = [];
  for (const part of parts) {
    if (part === "..") {
      if (normalized.length > 0) normalized.pop();
    } else if (part && part !== ".") normalized.push(part);
  }
  return `${path.startsWith("/") ? "/" : ""}${normalized.join("/")}`;
};
globalThis.__quenchResolveObjectEarly = (result, from, to, originalResolve) => {
  const fragmentOnly = globalThis.__quenchResolveStringFragmentOnly(from, to);
  if (fragmentOnly) return fragmentOnly;
  if (to === "." && globalThis.__quenchIsOpaqueTarget(from)) {
    return `${from.match(/^([A-Za-z][A-Za-z0-9+.-]*):/)[1]}:`;
  }
  const parsedObject = globalThis.__quenchResolveParsedObject(result, from, to);
  if (parsedObject) return parsedObject;
  const absoluteTarget = globalThis.__quenchResolveAbsoluteTarget(from, to);
  if (absoluteTarget) return absoluteTarget;
  const mailto = globalThis.__quenchResolveMailtoRelative(from, to);
  if (mailto) return mailto;
  const parsed = globalThis.__quenchResolveParsedAbsolute(result, from, to);
  if (parsed) return parsed;
  const file = globalThis.__quenchResolveLegacyFileRelative(from, to);
  if (file) return file;
  return /^[A-Za-z][A-Za-z0-9+.-]*:\/\/\//.test(from)
    ? globalThis.__quenchResolveEmptyAuthority(from, to)
    : null;
};
globalThis.__quenchAddLegacyParseMethods = (result) => {
  const originalParse = result.parse;
  result.parse = (input, ...args) => {
    const parsed = originalParse(input, ...args);
    const resolveObject = (target) => {
      if (/^javascript:/i.test(target)) return originalParse(target);
      const schemeTarget = String(target).match(
        /^([a-z][a-z0-9+.-]*):(\/\/)?(.*)$/i
      );
      if (schemeTarget && !schemeTarget[2]) {
        const [, scheme, , rest] = schemeTarget;
        if (rest.startsWith("#")) return originalParse(`${scheme}:///${rest}`);
        return originalParse(`${scheme}://${rest}`);
      }
      return result.parse(globalThis.__nodeLegacyResolve(parsed.href, target));
    };
    Object.defineProperties(parsed, {
      resolveObject: { value: resolveObject },
      resolve: { value: resolveObject }
    });
    return parsed;
  };
};
globalThis.__nodeLegacyMailtoParts = (input) => {
  if (!/^mailto:/i.test(input)) return null;
  const [address, query = ""] = input.slice(input.indexOf(":") + 1).split("?");
  const at = address.lastIndexOf("@");
  const auth = at < 0 ? null : address.slice(0, at);
  const host = at < 0 ? null : address.slice(at + 1);
  return {
    href: input,
    protocol: "mailto:",
    host,
    auth,
    hostname: host,
    search: query ? `?${query}` : null,
    query: query || null,
    path: query ? `?${query}` : null,
    slashes: null,
    port: null,
    hash: null
  };
};
globalThis.__nodeLegacySchemeAddressParts = (input) => {
  const match = input.match(
    /^([a-z][a-z0-9+.-]*:)([^/?#]*@[^/?#]*)(?:\?([^#]*))?/i
  );
  if (!match || ["mailto:", "javascript:"].includes(match[1].toLowerCase())) {
    return null;
  }
  const at = match[2].lastIndexOf("@");
  const auth = match[2].slice(0, at);
  const host = match[2].slice(at + 1);
  const search = match[3] ? `?${match[3]}` : null;
  return {
    href: input,
    protocol: match[1].toLowerCase(),
    host,
    auth,
    hostname: host,
    slashes: null,
    port: null,
    hash: null,
    search,
    query: match[3] || null,
    path: search
  };
};
globalThis.__nodeLegacyOpaquePathParts = (input) => {
  const match = input.match(
    /^([a-z][a-z0-9+.-]*:)([^/?#]+)\/([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/i
  );
  if (!match || match[1].toLowerCase() === "mailto:") return null;
  const search = match[4] ? `?${match[4]}` : null;
  const hash = match[5] ? `#${match[5]}` : null;
  return {
    href: input,
    host: match[2],
    hostname: match[2],
    protocol: match[1].toLowerCase(),
    pathname: `/${match[3]}`,
    path: `/${match[3]}${search || ""}`,
    slashes: null,
    auth: null,
    port: null,
    hash,
    search,
    query: match[4] || null
  };
};
globalThis.__nodeLegacyHostASCII = (host) => {
  try {
    return globalThis.require("punycode").toASCII(host);
  } catch (_) {
    return host;
  }
};
