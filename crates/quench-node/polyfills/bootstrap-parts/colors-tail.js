const __nodeURLResolveInput = (input, base) => {
  // prettier-ignore
  let value = (/^file:[a-z][|:][\\/]/i.test(
      String(input).trim().replace(/[\t\n\r]/g, ""),
    ) ||
      (/^\/{0,2}[a-z][|:][\\/]/i.test(
        String(input).trim().replace(/[\t\n\r]/g, ""),
      ) && /^file:/i.test(base || "")))
    ? `file:///${
      String(input).trim().replace(/[\t\n\r]/g, "").replace(/^file:/i, "")
        .replace(/^\/{0,2}(?=[a-z][|:])/i, "").replace(/^([a-z])\|/i, "$1:")
        .replace(/\\/g, "/")
    }`
    : /^(?:\\\\|\/\\)/.test(String(input).trim()) &&
        /^file:/i.test(base || "")
    ? String(input).trim().replace(/^(?:\\\\|\/\\)/, "//").replace(/\\/g, "/")
    : String(input).trim().replace(/[\t\n\r]/g, "");
  // prettier-ignore
  if (
    base &&
    (value === ""
      ? (value = String(base).trim().replace(/[\t\n\r]/g, ""))
      : /^(?:https?|ftp):\/\//i.test(base) &&
        (value = value.replace(/^[^?#]*/, (path) => path.replace(/\\/g, "/"))))
  );
  // prettier-ignore
  if (
    base &&
    (!/^[a-z][a-z0-9+.-]*:/.test(value) ||
      (/^https?:[^/]/.test(value) &&
        value.slice(0, value.indexOf(":")) ===
          String(base).slice(0, String(base).indexOf(":"))) ||
      (/^(?:https?):\/(?:[^/]|$)/.test(value) &&
        value.slice(0, value.indexOf(":")) ===
          String(base).slice(0, String(base).indexOf(":"))))
  ) {
    const baseUrl = new globalThis.__nodeURL(base);
    value = value.replace(/^(?:https?|ftp):/, "");
    if (/^[?#]/.test(value)) value = baseUrl.origin + baseUrl.pathname + value;
    else {value = value.startsWith("//")
        ? baseUrl.protocol + value
        : value.startsWith("/")
        ? baseUrl.origin + value
        : baseUrl.origin + baseUrl.pathname.replace(/\/[^/]*$/, "/") + value;}
  }
  // prettier-ignore
  return globalThis.__quenchNormalizeSpecialUrlInput(value).replace(
    /\/\.\//g,
    "/",
  );
};
// prettier-ignore
const __nodeURLCredentials = (authority) => {
  const at = authority.lastIndexOf("@"),
    raw = at < 0 ? "" : authority.slice(0, at),
    separator = raw.indexOf(":");
  return separator < 0
    ? [raw, ""]
    : [raw.slice(0, separator), raw.slice(separator + 1)];
};
// prettier-ignore
const __nodeURLNormalizeIPv4Tail = (host) =>
  host.replace(
    /^(\[[^\]]*?):?(\d+\.\d+\.\d+\.\d+)(\].*)$/,
    (_, prefix, ip, suffix) => {
      const octets = ip.split(".").map(Number),
        high = (octets[0] << 8) + octets[1],
        low = (octets[2] << 8) + octets[3];
      return `${prefix}${prefix.endsWith("::") ? "" : ":"}${
        high.toString(16)
      }:${low.toString(16)}${suffix}`;
    },
  ).replace(/^\[(?:0:){2,}([^\]]+)\]$/, "[::$1]");
// eslint-disable-next-line complexity
const __nodeURLAssignParts = (url, match) => {
  const [username, password] = __nodeURLCredentials(match[2] || "");
  // prettier-ignore
  ((url.protocol = match[1] || ""),
    (url.host = __nodeURLNormalizeIPv4Tail(
      (match[2] || "").replace(/^.*@/, ""),
    )),
    (url._username = username.replace(/]/g, "%5D").replace(/:/g, "%3A")),
    (url._password = password.replace(/]/g, "%5D").replace(/:/g, "%3A").replace(
      /@/g,
      "%40",
    )),
    Object.defineProperty(url, "_hostname", {
      configurable: true,
      value: url.host.startsWith("[")
        ? url.host.slice(0, url.host.indexOf("]") + 1)
        : url.host.split(":")[0],
      writable: true,
    }),
    (url.port = url.host.startsWith("[")
      ? url.host.match(/^\[[^\]]*\](?::(.*))?$/)?.[1] || ""
      : url.host.includes(":")
      ? url.host.slice(url.host.lastIndexOf(":") + 1).replace(/^0+(?=\d)/, "")
      : ""));
  if (["http:80", "https:443"].includes(url.protocol + url.port)) url.port = "";
  // prettier-ignore
  url.pathname = match[3] || "/",
    url.search = match[4] !== undefined ? `?${match[4]}` : "",
    url.hash = match[5] !== undefined ? `#${match[5]}` : "",
    match[1] && !match[2] && (url._pathname = match[3] || "");
  // prettier-ignore
  Object.defineProperty(url, "origin", {
    configurable: true,
    enumerable: false,
    value: url.protocol && url.host ? `${url.protocol}//${url.host}` : "null",
    writable: true,
  }),
    Object.defineProperty(url, "searchParams", {
      configurable: true,
      enumerable: false,
      value: (() => {
        const params = new NodeURLSearchParams(match[4] || "");
        Object.defineProperty(params, "__nodeURLOwner", {
          configurable: true,
          value: url,
          writable: true,
        });
        return params;
      })(),
      writable: true,
    }),
    url._origin = url.origin,
    url._searchParams = url.searchParams,
    delete url.origin,
    delete url.searchParams;
  // prettier-ignore
  Object.keys(url).filter((key) => key.startsWith("_")).forEach((key) =>
    Object.defineProperty(url, key, { enumerable: false })
  );
};
globalThis.__nodeURL = class NodeURL {
  constructor(input, base) {
    const value = __nodeURLResolveInput(input, base);
    // prettier-ignore
    const match = value.match(
      /^([a-z][a-z0-9+.-]*:)?(?:\/\/([^/?#]*))?([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/i,
    );
    // prettier-ignore
    if (
      input === undefined || base === null ||
      (!base && !/^[a-z][a-z0-9+.-]*:/i.test(value)) || !match
    ) {
      throw Object.assign(new TypeError("Invalid URL"), {
        code: "ERR_INVALID_URL",
      });
    }
    // prettier-ignore
    __nodeURLAssignParts(this, match),
      /^[a-z][a-z0-9+.-]*:\/\//i.test(value) &&
      Object.defineProperty(this, "_hasAuthorityDelimiter", {
        configurable: true,
        value: true,
        writable: true,
      });
    // prettier-ignore
    return new Proxy(this, {
      get: (target, property, receiver) => {
        const value = Reflect.get(target, property, receiver);
        if (property === "searchParams") value.__nodeURLOwner = receiver;
        return value;
      },
      set: (target, property, value, receiver) =>
        property === "origin" || property === "searchParams"
          ? globalThis.__nodeThrowReadonlyURLSetter(property)
          : Reflect.set(target, property, value, receiver),
    });
  }
  // eslint-disable-next-line complexity
  get href() {
    if (!(this instanceof globalThis.__nodeURL)) {
      throw new TypeError("Receiver must be an instance of class URL");
    }
    // prettier-ignore
    const credentials = this.username || this.password
        ? `${this.username || ""}${this.password ? `:${this.password}` : ""}@`
        : "",
      prefix = this.protocol === "file:"
        ? `file://${this.host === "localhost" ? "" : this.host}${
          this.pathname === "" ? "/" : ""
        }`
        : this.origin === "null"
        ? this.protocol +
          (this._hasAuthorityDelimiter ? `//${credentials}${this.host}` : "")
        : this.origin.replace(/^(\w+:\/\/)/, `$1${credentials}`);
    return `${prefix}${this.pathname}${this.search}${this.hash}`;
  }
  // prettier-ignore
  toString() {
    if (!(this instanceof globalThis.__nodeURL)) {
      throw new TypeError("Receiver must be an instance of class URL");
    }
    return this.href;
  }
};
// prettier-ignore
Object.defineProperty(globalThis, "URL", {
  configurable: true,
  enumerable: false,
  value: globalThis.__nodeURL,
  writable: true,
}),
  Object.defineProperty(globalThis, "URLSearchParams", {
    configurable: true,
    enumerable: false,
    value: globalThis.__nodeURLSearchParams,
    writable: true,
  });
const __nodeLegacyUrlHostInvalid = (host) =>
  !/^\[[^\]]+\](?::\d*)?$/.test(host) && /[#/?@[\\\]^|]/.test(host);
const __nodeLegacyUrlValidateAuthority = (value) => {
  const rawAuthority = value.match(/^[a-z][a-z0-9+.-]*:\/\/([^/?#]+)/i)?.[1] ||
    "";
  const host = rawAuthority.slice(rawAuthority.lastIndexOf("@") + 1);
  const hostnameForIdna = host.startsWith("[")
    ? host.slice(0, host.indexOf("]") + 1)
    : host.split(":")[0];
  const idnaInvalid = !hostnameForIdna.startsWith("[") &&
    Array.from(hostnameForIdna).some((character) =>
      character.normalize("NFKD").match(/[#%/:?@[\\\]^|]/)
    );
  if (
    /[\u0000\u00ad]/.test(rawAuthority) ||
    idnaInvalid ||
    __nodeLegacyUrlHostInvalid(host)
  ) {
    const error = new TypeError("Invalid URL");
    error.code = "ERR_INVALID_URL";
    error.input = value;
    throw error;
  }
};
const __nodeLegacyUrlAuthority = (input) => {
  const authority =
    input.match(/^(?:[a-z][a-z0-9+.-]*:)?\/\/([^/?#]*)/i)?.[1] || "";
  const at = authority.lastIndexOf("@");
  const auth = at >= 0 ? decodeURIComponent(authority.slice(0, at)) : null;
  const host = globalThis.__nodeLegacyHostASCII(
    (at >= 0 ? authority.slice(at + 1) : authority).toLowerCase(),
  );
  return { auth, host };
};
const __nodeLegacyUrlPathParts = (parsed, host, input) => {
  const authorityPath = input.match(
    /^[a-z][a-z0-9+.-]*:\/\/[^/?#]*(\/[^?#]*)?/i,
  )?.[1];
  const singleSlashPath = input.match(
    /^[a-z][a-z0-9+.-]*:((?!\/\/)[^?#]*)/i,
  )?.[1];
  const parsedPathname = authorityPath ?? singleSlashPath ?? parsed.pathname;
  const pathname = parsedPathname?.startsWith("/;")
    ? parsedPathname.slice(1)
    : parsedPathname || (host ? "/" : "");
  const search = parsed.search || null;
  return { pathname, search };
};
const __nodeLegacyUrlValue = (value) => value || null;
const __nodeLegacyUrlPathValue = (pathname, search) =>
  pathname ? `${pathname}${search || ""}` : null;
const __nodeLegacyUrlParts = (input, parsed) => {
  const protocol = parsed.protocol.toLowerCase();
  const { auth, host } = __nodeLegacyUrlAuthority(input);
  const hostname = host.startsWith("[")
    ? host.slice(1, host.indexOf("]"))
    : host.split(":")[0];
  const port = host.match(/:(\d+)$/)?.[1] || null;
  const { pathname: rawPathname, search: parsedSearch } =
    __nodeLegacyUrlPathParts(parsed, host, input);
  const rawQuery = input.split("#", 1)[0].match(/\?([^#]*)/)?.[1];
  const search = rawQuery === undefined
    ? parsedSearch
    : `?${globalThis.__nodeLegacyQueryNormalize(rawQuery)}`;
  const pathname = __nodeLegacyPathname(input, protocol, host, rawPathname);
  const hrefPath = globalThis.__nodeLegacyUrlHrefPath(pathname);
  const hrefAuth = auth
    ? `${auth.replace(/[" @<]/g, encodeURIComponent)}@`
    : "";
  return {
    protocol: __nodeLegacyUrlValue(protocol),
    slashes: globalThis.__nodeLegacyUrlSlashes(input, protocol) ? true : null,
    auth,
    host: globalThis.__nodeLegacyUrlHostValue(protocol, host),
    port,
    hostname: globalThis.__nodeLegacyUrlHostValue(protocol, hostname),
    hash: input.includes("#")
      ? `#${input.split("#").slice(1).join("#")}`
      : null,
    search,
    query: search ? search.slice(1) : null,
    pathname: __nodeLegacyUrlValue(pathname),
    path: __nodeLegacyUrlPathValue(pathname, search),
    href: `${
      globalThis.__nodeLegacyUrlHrefPrefix(
        protocol,
        hrefAuth,
        host,
      )
    }${hrefPath}${search || ""}${
      input.includes("#") ? `#${input.split("#").slice(1).join("#")}` : ""
    }`,
  };
};
const __nodeLegacyPathname = (input, protocol, host, pathname) =>
  !protocol && !host && pathname === input
    ? globalThis.__nodeLegacyPathEncode(pathname)
    : pathname;
const __nodeLegacyUrlFormatSearch = (value) => {
  if (value.search !== undefined) return value.search;
  if (value.query === undefined) return "";
  return typeof value.query === "string" ? `?${value.query}` : "";
};
const __nodeLegacyUrlFormatPrefix = (protocol, value, host) =>
  protocol && (value.slashes || host) ? `${protocol}//` : protocol;
const __nodeLegacyUrlFormatPath = (value, host) =>
  value.pathname || (host ? "/" : "");
const __nodeLegacyUrlFormatObject = (value) => {
  const protocol = value.protocol || "";
  const host = value.host || value.hostname || "";
  const prefix = __nodeLegacyUrlFormatPrefix(protocol, value, host);
  const pathname = __nodeLegacyUrlFormatPath(value, host);
  const search = __nodeLegacyUrlFormatSearch(value);
  return `${prefix}${host}${pathname}${search || ""}${value.hash || ""}`;
};
const __nodeUrlModuleExports = {
  URL: globalThis.__nodeURL,
  URLSearchParams: globalThis.__nodeURLSearchParams,
  urlToHttpOptions: (value) => globalThis.__nodeUrlToHttpOptions(value),
  fileURLToPath: (value) => {
    let href;
    if (value && typeof value.href === "string") href = value.href;
    else href = String(value);
    if (!href.startsWith("file://")) {
      throw new TypeError("URL must be a file URL");
    }
    let p = decodeURIComponent(href.slice("file://".length));
    return p || "/";
  },
  pathToFileURL: (value, options) => {
    const windows = options && options.windows;
    __nodeValidateWindowsFileHost(value, windows);
    const specialWindowsURL = __nodeWindowsSpecialFileURL(value, windows);
    if (specialWindowsURL) return specialWindowsURL;
    const sep = windows ? "\\" : "/";
    const resolved = globalThis.__nodePath.resolve(String(value));
    const isAbsolute = globalThis.__nodePath.isAbsolute(resolved);
    let p = resolved.split(sep).join("/");
    if (windows && /^[A-Za-z]:/.test(p)) p = "/" + p;
    const input = String(value);
    const trailing = input.endsWith(sep) || input.endsWith("/");
    p = p
      .split("/")
      .map((seg, i) => {
        if (i === 0) return seg;
        return encodeURIComponent(seg)
          .replace(/%26/g, "&")
          .replace(/%24/g, "$")
          .replace(/%2B/gi, "+")
          .replace(/%2C/gi, ",")
          .replace(/%3D/gi, "=")
          .replace(/%3A/gi, ":")
          .replace(/%3B/gi, ";")
          .replace(/~/g, "%7E");
      })
      .join("/");
    if (trailing && !p.endsWith("/")) p = p + "/";
    return new globalThis.__nodeURL("file://" + (isAbsolute ? "" : "") + p);
  },
  parse: (value, parseQueryString = false) => {
    if (typeof value !== "string") globalThis.__nodePrepareLegacyUrl(value);
    const raw = typeof value === "string"
      ? value.trim().replace(/^[\x00-\x20]+|[\x00-\x20]+$/g, "")
      : value;
    const earlyProtocolRelative = typeof raw === "string" &&
      globalThis.__nodeLegacyProtocolRelativeParts(raw);
    if (earlyProtocolRelative) return earlyProtocolRelative;
    if (typeof raw === "string" && raw.startsWith("//")) {
      const synthetic = `http:${raw}`;
      __nodeLegacyUrlValidateAuthority(synthetic);
      const parts = __nodeLegacyUrlParts(
        synthetic,
        new globalThis.__nodeURL(synthetic),
      );
      parts.protocol = null;
      parts.href = parts.href.slice("http:".length);
      return parts;
    }
    if (typeof raw === "string" && /^\[[^\]]+\](?:[/?#]|$)/.test(raw)) {
      return globalThis.__nodeLegacyPathOnlyParts(raw);
    }
    if (
      typeof raw === "string" &&
      !/^[a-z][a-z0-9+.-]*:/i.test(raw) &&
      !raw.startsWith("//")
    ) {
      const hashIndex = raw.indexOf("#");
      const withoutHash = hashIndex < 0 ? raw : raw.slice(0, hashIndex);
      const queryIndex = withoutHash.indexOf("?");
      const pathname = queryIndex < 0
        ? withoutHash
        : withoutHash.slice(0, queryIndex);
      const search = queryIndex < 0
        ? null
        : globalThis.__nodeLegacyQueryNormalize(
          withoutHash.slice(queryIndex),
        );
      const hash = hashIndex < 0
        ? null
        : globalThis.__nodeLegacyQueryNormalize(raw.slice(hashIndex));
      const parsed = globalThis.__nodeLegacyPathOnlyParts(
        globalThis.__nodeLegacyPathEncode(pathname),
      );
      parsed.search = search;
      parsed.query = search?.slice(1) || null;
      parsed.path = `${parsed.pathname}${search || ""}`;
      parsed.hash = hash;
      parsed.href = `${parsed.path}${hash || ""}`;
      return parsed;
    }
    const prepared = globalThis.__nodePrepareLegacyUrl(value);
    __nodeLegacyUrlValidateAuthority(prepared.input);
    const { input, parsed } = prepared;
    const protocolRelative = globalThis.__nodeLegacyProtocolRelativeParts(
      input,
    );
    if (protocolRelative) return protocolRelative;
    const mailto = globalThis.__nodeLegacyMailtoParts(input);
    if (mailto) return mailto;
    const schemeAddress = globalThis.__nodeLegacySchemeAddressParts(input);
    if (schemeAddress) return schemeAddress;
    const opaquePath = globalThis.__nodeLegacyOpaquePathParts(input);
    if (opaquePath) return opaquePath;
    return __nodeLegacyUrlParts(input, parsed);
  },
  format: (value) => {
    if (value instanceof globalThis.__nodeURL) return value.href;
    if (typeof value === "string") return __nodeLegacyUrlFormatString(value);
    if (value && typeof value === "object") {
      return __nodeLegacyUrlFormatObject(value);
    }
    return String(value);
  },
  resolve: (from, to) => globalThis.__nodeLegacyResolve(from, to),
};
let __nodeUrlModuleInstance;
globalThis.__nodeUrlModule = new Proxy(
  {},
  {
    get: (_, key) => {
      return (__nodeUrlModuleInstance ||= __nodeUrlModuleExports)[key];
    },
    ownKeys: () => Reflect.ownKeys(__nodeUrlModuleExports),
    getOwnPropertyDescriptor: (_, key) => ({
      enumerable: true,
      configurable: true,
      value: __nodeUrlModuleExports[key],
    }),
  },
);
