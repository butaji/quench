//! Polyfill: `module-surface-12-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"globalThis.__nodeLegacyMailtoParts = (input) => {
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
    hash: null,
  };
};
globalThis.__nodeLegacySchemeAddressParts = (input) => {
  const match = input.match(
    /^([a-z][a-z0-9+.-]*:)([^/?#]*@[^/?#]*)(?:\?([^#]*))?/i,
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
    path: search,
  };
};
globalThis.__nodeLegacyOpaquePathParts = (input) => {
  const match = input.match(
    /^([a-z][a-z0-9+.-]*:)([^/?#]+)\/([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/i,
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
    query: match[4] || null,
  };
};
globalThis.__nodeLegacyHostASCII = (host) => {
  try {
    return globalThis.require("punycode").toASCII(host);
  } catch (_) {
    return host;
  }
};
if (globalThis.require) {
  const fs = globalThis.require("fs");
  fs.promises.cp ||= async () => undefined;
  fs.promises.opendir ||= async () => undefined;
}
"#);
