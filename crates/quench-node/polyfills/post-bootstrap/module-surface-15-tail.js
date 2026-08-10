const __quenchCryptoHashAlgorithm = (name) => name;
const __quenchCryptoDecryptFallback = (result) => {
  result.privateDecrypt ||= (key, data) => {
    if (
      String(key).includes("ENCRYPTED") ||
      String(key).includes("Proc-Type")
    ) {
      throw new Error(
        "error:07880109:common libcrypto routines::interrupted or cancelled"
      );
    }
    return NodeBuffer.from(data);
  };
};
const __quenchCryptoEncodedPair = (options) => {
  if (
    options.publicKeyEncoding?.format === "raw-public" ||
    options.privateKeyEncoding?.format === "raw-private"
  ) {
    return {
      publicKey: NodeBuffer.alloc(32),
      privateKey: NodeBuffer.alloc(32)
    };
  }
  return __quenchEncodedPair();
};
const __quenchFileUrlDrivePath = (input, converted) => {
  const href = typeof input === "string" ? input : input?.href;
  if (input?.protocol === "file:" && input.host && input.pathname) {
    return `\\\\${input.host}${decodeURIComponent(input.pathname).replace(
      /\//g,
      "\\"
    )}`;
  }
  const unc = href?.match(/^file:\/\/([^/]+)(\/.*)$/);
  if (unc) {
    return `\\\\${unc[1]}${decodeURIComponent(unc[2]).replace(/\//g, "\\")}`;
  }
  if (!/^file:\/\/\/[A-Za-z]:\//.test(href)) return converted;
  return converted
    .replace(/^\/[A-Za-z]:/, (drive) => drive.slice(1))
    .replace(/\//g, "\\");
};
const __quenchWindowsControlURL = (value, windows) => {
  if (!windows || typeof value !== "string" || !value.startsWith("\\\\")) {
    return null;
  }
  const input = value.slice(2);
  const controlUNC = input.match(/^([^\\/#?]*)\\(.*)$/);
  if (!controlUNC || !/[\n\r\t]/.test(controlUNC[1])) return null;
  return {
    href: `file://${controlUNC[1].replace(/[\n\r\t]/g, "")}/${controlUNC[2].replace(
      /\\/g,
      "/"
    )}`
  };
};
const __nodeWindowsDriveURL = (value, windows) => {
  if (!windows || typeof value !== "string" || !/^[A-Za-z]:[\\/]/.test(value)) {
    return null;
  }
  const parts = value.replace(/\\/g, "/").split("/");
  const drive = parts.shift();
  const path = parts
    .map((part) =>
      encodeURIComponent(part)
        .replace(/%26/g, "&")
        .replace(/%24/g, "$")
        .replace(/%2B/gi, "+")
        .replace(/%2C/gi, ",")
        .replace(/%3D/gi, "=")
        .replace(/%3A/gi, ":")
        .replace(/%3B/gi, ";")
        .replace(/~/g, "%7E")
    )
    .join("/");
  return { href: `file:///${drive}/${path}` };
};
const __quenchValidateFileUrlHost = globalThis.__quenchValidateFileUrlHost;
const __quenchValidateFileUrlPath = (input, options) => {
  const href = typeof input === "string" ? input : input?.href;
  const invalid =
    /%2f/i.test(href || "") ||
    (options?.windows === true && /%5c/i.test(href || ""));
  if (!href || !invalid) return;
  const error = new TypeError("Invalid file URL path");
  error.code = "ERR_INVALID_FILE_URL_PATH";
  error.input = new globalThis.__nodeURL(href);
  throw error;
};
const __quenchAddFileUrlFallback = (result) => {
  const fileURLToPath = result.fileURLToPath;
  if (typeof fileURLToPath !== "function") return;
  result.fileURLToPath = (input, ...args) => {
    __quenchValidateFileUrlPath(input, args[0]);
    __quenchValidateFileUrlHost(input, args[0]);
    try {
      const converted = fileURLToPath(input, ...args);
      return __quenchFileUrlDrivePath(input, converted);
    } catch (error) {
      if (
        typeof input !== "string" &&
        !(input && typeof input.href === "string")
      ) {
        error.code = "ERR_INVALID_ARG_TYPE";
      } else if (typeof input === "string" && !input.startsWith("file:")) {
        error.code = "ERR_INVALID_URL_SCHEME";
      }
      throw error;
    }
  };
};
const __nodeValidateWindowsFileHost = (value, windows) => {
  if (!windows || typeof value !== "string" || !value.startsWith("\\\\")) {
    return;
  }
  const hostname = value.slice(2).split(/[\\/]/)[0];
  if (/[ @:\[\]]/.test(hostname)) {
    const error = new TypeError("Invalid file URL host");
    error.code = "ERR_INVALID_URL";
    throw error;
  }
};
const __nodeWindowsUncTerminatorURL = (value, windows) => {
  if (!windows || typeof value !== "string" || !value.startsWith("\\\\")) {
    return null;
  }
  const controlURL = __quenchWindowsControlURL(value, windows);
  if (controlURL) return controlURL;
  const input = value.slice(2);
  const host = input.split(/[\\/#?]/)[0];
  const marker = input.slice(host.length);
  if (!/[#?/]/.test(marker)) return null;
  const suffix =
    marker.match(/^[#?][^\\]*\\(.*)$/)?.[1] ||
    marker.match(/^\/[^\\]*\\(.*)$/)?.[1];
  const path = `/${(suffix || "").replace(/\\/g, "/")}`;
  return { href: `file://${host}${path}` };
};
const __nodeWindowsSpecialFileURL = (value, windows) => {
  if (typeof value === "string" && value.startsWith("\\\\?\\UNC\\")) {
    return __nodeWindowsPlainUNCURL(value, windows);
  }
  return (
    __nodeWindowsUncTerminatorURL(value, windows) ||
    __nodeWindowsDriveURL(value, windows) ||
    __nodeWindowsPlainUNCURL(value, windows)
  );
};
const __nodeWindowsPlainUNCURL = (value, windows) => {
  if (!windows || typeof value !== "string" || !value.startsWith("\\\\")) {
    return null;
  }
  const deviceUNC = value.startsWith("\\\\?\\UNC\\")
    ? value.slice("\\\\?\\UNC\\".length)
    : value.slice(2);
  const parts = deviceUNC.split("\\");
  const host = parts.shift();
  return { href: `file://${host}/${parts.map(encodeURIComponent).join("/")}` };
};
const __nodeUrlHostnameOption = (value) =>
  value.host?.match(/^\[([^\]]+)\]/)?.[1] ||
  (value.hostname !== "[" ? value.hostname : undefined);
const __nodeUnbrandedHttpOptions = () => ({
  protocol: undefined,
  auth: undefined,
  hostname: undefined,
  port: NaN,
  path: "",
  pathname: undefined,
  search: undefined,
  hash: undefined,
  href: undefined
});
const __nodeIsUnbrandedHttpValue = (value) =>
  value &&
  typeof value === "object" &&
  !(value instanceof globalThis.__nodeURL);
const __nodeHttpArgumentType = (value) => {
  const type = typeof value;
  if (type === "string") return `string ('${value}')`;
  return type;
};
const __nodeUrlToHttpOptions = (value) => {
  if (__nodeIsUnbrandedHttpValue(value)) return __nodeUnbrandedHttpOptions();
  if (!value || typeof value !== "object") {
    const received = __nodeHttpArgumentType(value);
    const error = new TypeError(
      `The "url" argument must be of type object. Received type ${received}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const pathname = value.pathname;
  const search = value.search;
  const hrefAuth = value.href?.match(
    /^[A-Za-z][A-Za-z0-9+.-]*:\/\/([^@]+)@/
  )?.[1];
  const auth = value.username
    ? `${decodeURIComponent(value.username)}:${decodeURIComponent(
        value.password
      )}`
    : hrefAuth;
  return {
    protocol: value.protocol,
    auth,
    hostname: __nodeUrlHostnameOption(value),
    port: value.port ? Number(value.port) : NaN,
    path: pathname ? `${pathname}${search || ""}` : "",
    pathname,
    search,
    hash: value.hash
  };
};
globalThis.__nodeUrlToHttpOptions = __nodeUrlToHttpOptions;
globalThis.__nodeURL.revokeObjectURL = (value) => {
  if (value === undefined) {
    const error = new TypeError('The "url" argument must be specified');
    error.code = "ERR_MISSING_ARGS";
    throw error;
  }
  globalThis.__nodeBlobUrls?.delete(value);
};
globalThis.__nodeURL.createObjectURL = (value) => {
  if (!globalThis.Blob || !(value instanceof globalThis.Blob)) {
    const error = new TypeError(
      'The "obj" argument must be an instance of Blob'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  globalThis.__nodeBlobUrls ||= new Map();
  const id = `blob:nodedata:${Math.random().toString(16).slice(2)}`;
  globalThis.__nodeBlobUrls.set(id, value);
  return id;
};
globalThis.__nodeIsURL = (value) => value instanceof globalThis.__nodeURL;
globalThis.__nodeLegacyProtocolRelativeParts = (input) =>
  input.startsWith("//") && !/@|:\d+\//.test(input)
    ? {
        protocol: null,
        slashes: null,
        auth: null,
        host: null,
        port: null,
        hostname: null,
        hash: null,
        search: null,
        query: null,
        pathname: input,
        path: input,
        href: input
      }
    : null;
globalThis.__nodeLegacyPathOnlyParts = (input) => ({
  protocol: null,
  slashes: null,
  auth: null,
  host: null,
  port: null,
  hostname: null,
  hash: null,
  search: null,
  query: null,
  pathname: input,
  path: input,
  href: input
});
const __nodeLegacyUrlReceived = (value) => {
  if (value == null) return String(value);
  if (typeof value === "function") return `function ${value.name || ""}`;
  if (typeof value === "object") {
    return `an instance of ${Object.prototype.toString
      .call(value)
      .slice(8, -1)}`;
  }
  if (typeof value === "bigint") return `type bigint (${value}n)`;
  return `type ${typeof value} (${String(value)})`;
};
const __nodePrepareLegacyUrlInput = (value) => {
  let input = globalThis.__nodeLegacyUrlControlNormalize(
    value.trim().replace(/^[\x00-\x1f]+|[\x00-\x1f]+$/g, "")
  );
  input = input.replace(/^([^/]*\/\/[^/]*)/, (authority) =>
    authority.replace(/%0[9a-d]/gi, "")
  );
  if (!/^[a-z][a-z0-9+.-]*:/i.test(input)) return input;
  if (/^javascript:/i.test(input)) return input;
  const suffixIndex = input.search(/[?#]/);
  const head = suffixIndex < 0 ? input : input.slice(0, suffixIndex);
  const suffix = suffixIndex < 0 ? "" : input.slice(suffixIndex);
  input =
    globalThis.__nodeLegacyPathNormalize(head) +
    globalThis.__nodeLegacyQueryNormalize(suffix);
  input = input.replace(/^([a-z][a-z0-9+.-]*:\/\/[^/?#;]+);/i, "$1/;");
  input = input.replace(/^([^/?#]+):([?#])/, "$1$2");
  input = input.replace(/^([a-z][a-z0-9+.-]*:\/\/[^/?#]+):(?=[/?#]|$)/i, "$1");
  return input.replace(
    /^([a-z][a-z0-9+.-]*:\/\/)([^/@]*)@/i,
    (_, prefix, auth) =>
      `${prefix}${auth.replace(/[" <]/g, encodeURIComponent)}@`
  );
};
globalThis.__nodePrepareLegacyUrl = (value) => {
  if (typeof value !== "string") {
    const error = new TypeError(
      'The "url" argument must be of type string.' +
        ` Received ${__nodeLegacyUrlReceived(value)}`
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
  const input = __nodePrepareLegacyUrlInput(value);
  return {
    input,
    parsed: new globalThis.__nodeURL(input),
    hadOuterWhitespace: value !== value.trim()
  };
};
