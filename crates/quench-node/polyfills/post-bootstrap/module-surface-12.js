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
  result.parentPort ??= null;
  result.workerData ??= undefined;
  result.threadId ??= 0;
};
const __quenchFsModuleFallbacks = (result) => {
  __quenchSetFallbacks(
    result,
    ["glob", "cp", "cpSync", "watch", "watchFile", "unwatchFile"],
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
    .replace(/\?/g, "%3F");
  if (authority && pathname && !pathname.startsWith("/"))
    pathname = `/${pathname}`;
  if (
    authority &&
    input.protocol !== "mailto:" &&
    !pathname &&
    (input.search != null || input.query != null)
  )
    pathname = "/";
  return pathname;
};
globalThis.__quenchResolveEmptyAuthority = (from, to) => {
  if (/^[a-z][a-z0-9+.-]*:/i.test(to)) return to;
  if (to.startsWith("/"))
    return `${from.match(/^[a-z][a-z0-9+.-]*:\/\//i)?.[0] || ""}${to.startsWith("//") ? to.slice(2) : to}`.replace(
      /^http:\/\/\//,
      "http:/"
    );
  let base = from.replace(/[^/]*$/, "");
  let target = to;
  let parentTraversal = 0;
  while (target.startsWith("../")) {
    parentTraversal += 1;
    base = base.replace(/[^/]+\/$/, "");
    target = target.slice(3);
  }
  if (parentTraversal > 3) base = base.replace(/[^/]+\/$/, "");
  if (parentTraversal > 2)
    base = base.replace(
      /^(.*?:\/\/)(.*)$/,
      (_, prefix, path) => prefix + path.replace(/\/{2,}/g, "/")
    );
  if (parentTraversal > 3) base = base.replace(/[^/]+\/$/, "");
  return base.replace(/^http:\/\/\//, "http:/") + target.replace(/^\.\//, "");
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
  if (!match || ["mailto:", "javascript:"].includes(match[1].toLowerCase()))
    return null;
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
