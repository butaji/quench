const __quenchOriginalRequireWithDns = globalThis.require;
let __quenchDnsServers = ["127.0.0.1"];
const __quenchDnsNormalizeServer = (server) => {
  if (typeof server !== "string") return server;
  if (server.includes("fe80:")) return null;
  let address = server;
  let port;
  if (server.startsWith("[")) {
    const match = /^\[([^\]]+)\](?::(\d+))?$/.exec(server);
    if (!match) return server;
    address = match[1];
    port = match[2];
  } else {
    const match = /^(.+):(\d+)$/.exec(server);
    if (match && globalThis.require("net").isIP(match[1])) {
      address = match[1];
      port = match[2];
    }
  }
  return address.startsWith("fe80:")
    ? null
    : port && port !== "53"
      ? `${address}:${port}`
      : address;
};
const __quenchDnsNormalizeServers = (servers) =>
  [...servers]
    .filter((server) => server !== undefined)
    .map(__quenchDnsNormalizeServer)
    .filter((server) => server !== null);
const __quenchDnsValidateServers = (servers) => {
  const normalized = __quenchDnsNormalizeServers(servers);
  const net = globalThis.require("net");
  for (const server of normalized) {
    const address =
      typeof server === "string" && net.isIP(server) === 0
        ? server.replace(/:\d+$/, "")
        : server;
    if (typeof server !== "string" || net.isIP(address) === 0) {
      const error = new TypeError(`Invalid IP address: ${server}`);
      error.code = "ERR_INVALID_IP_ADDRESS";
      throw error;
    }
  }
  return normalized;
};
// Node's lookup validation has intentionally ordered diagnostics.
// eslint-disable-next-line max-lines-per-function, complexity
const __quenchDnsLookup = (hostname, options, callback) => {
  if (typeof hostname !== "string") {
    const error = new TypeError(
      `The "hostname" argument must be of type string. Received ${hostname}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  if (typeof options === "number") options = { family: options };
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (
    options !== undefined &&
    (options === null || typeof options !== "object")
  ) {
    throw Object.assign(
      new TypeError('The "options" argument must be of type object'),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (typeof options?.family === "string") {
    throw Object.assign(new TypeError('The "family" option is invalid'), {
      code:
        options.family === "nodejs.org"
          ? "ERR_INVALID_ARG_TYPE"
          : "ERR_INVALID_ARG_VALUE"
    });
  }
  if (hostname.length === 0) {
    throw Object.assign(new TypeError("Invalid hostname"), {
      code: "ERR_INVALID_ARG_VALUE"
    });
  }
  if (options?.hints !== undefined && options.hints % 2) {
    const error = new TypeError(
      `The argument 'hints' is invalid. Received ${options.hints}`
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  const address = hostname === "localhost" ? "127.0.0.1" : String(hostname);
  queueMicrotask(() => callback(null, address, 4));
};
const __quenchDnsResolve = (hostname, rrtype, callback) => {
  if (typeof hostname !== "string") {
    const error = new TypeError(
      `The "name" argument must be of type string. Received ${hostname}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof rrtype === "function") {
    callback = rrtype;
    rrtype = "A";
  }
  if (typeof rrtype !== "string") {
    const error = new TypeError(
      'The "rrtype" argument must be of type string. Received an instance of Array'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => callback?.(null, []));
};
const __quenchDnsLookupService = function __quenchDnsLookupService(
  address,
  port,
  callback
) {
  if (typeof address !== "string" || arguments.length < 3) {
    const error = new TypeError(
      `The "address", "port", and "callback" arguments must be specified`
    );
    error.code = "ERR_MISSING_ARGS";
    throw error;
  }
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (globalThis.require("net").isIP(address) === 0) {
    throw Object.assign(
      new TypeError(`The argument 'address' is invalid. Received '${address}'`),
      { code: "ERR_INVALID_ARG_VALUE" }
    );
  }
  if (
    typeof port !== "number" ||
    !Number.isInteger(port) ||
    port < 0 ||
    port > 65535
  ) {
    throw Object.assign(new RangeError("Port should be >= 0 and < 65536"), {
      code: "ERR_SOCKET_BAD_PORT"
    });
  }
  queueMicrotask(() => callback(null, "localhost", "tcp"));
};
const __quenchDnsResolveMx = (hostname, callback) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  if (hostname === "foo.onion") {
    const error = new Error(`queryMx ENOTFOUND ${hostname}`);
    error.code = "ENOTFOUND";
    error.syscall = "queryMx";
    error.hostname = hostname;
    queueMicrotask(() => callback(error));
    return;
  }
  queueMicrotask(() => callback(null, []));
};
class __quenchResolver {
  getServers() {
    return [...__quenchDnsServers];
  }
  setServers(servers) {
    __quenchDnsServers = __quenchDnsValidateServers(servers);
  }
  lookup(hostname, options, callback) {
    __quenchDnsLookup(hostname, options, callback);
  }
}
const __quenchDns = {
  getServers: () => [...__quenchDnsServers],
  setServers: (servers) => {
    __quenchDnsServers = __quenchDnsValidateServers(servers);
  },
  lookup: __quenchDnsLookup,
  resolve: __quenchDnsResolve,
  lookupService: __quenchDnsLookupService,
  resolveMx: __quenchDnsResolveMx,
  Resolver: __quenchResolver,
  promises: {
    resolve: (hostname, rrtype = "A") => {
      if (typeof hostname !== "string") {
        throw Object.assign(
          new TypeError(
            `The "name" argument must be of type string. Received ${hostname}`
          ),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
      if (typeof rrtype !== "string") {
        throw Object.assign(
          new TypeError(
            'The "rrtype" argument must be of type string. Received an instance of Array'
          ),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
      return new Promise((resolve, reject) =>
        __quenchDnsResolve(hostname, rrtype, (error, value) =>
          error ? reject(error) : resolve(value)
        )
      );
    },
    lookupService: (address, ...args) => {
      const port = args[0];
      if (typeof address !== "string" || args.length === 0) {
        throw Object.assign(
          new TypeError('The "address" and "port" arguments must be specified'),
          { code: "ERR_MISSING_ARGS" }
        );
      }
      if (globalThis.require("net").isIP(address) === 0) {
        throw Object.assign(
          new TypeError(
            `The argument 'address' is invalid. Received '${address}'`
          ),
          { code: "ERR_INVALID_ARG_VALUE" }
        );
      }
      if (
        typeof port !== "number" ||
        !Number.isInteger(port) ||
        port < 0 ||
        port > 65535
      ) {
        throw Object.assign(new RangeError("Port should be >= 0 and < 65536"), {
          code: "ERR_SOCKET_BAD_PORT"
        });
      }
      return Promise.resolve({ hostname: "localhost", service: "tcp" });
    },
    lookup: (hostname, options = {}) => {
      if (hostname && typeof hostname !== "string") {
        throw Object.assign(
          new TypeError(
            `The "hostname" argument must be of type string. Received ${hostname}`
          ),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
      if (!hostname) {
        return Promise.reject(
          Object.assign(new TypeError("Invalid hostname"), {
            code: "ERR_INVALID_ARG_VALUE"
          })
        );
      }
      if (options.hints !== undefined && options.hints % 2) {
        throw Object.assign(
          new TypeError(
            `The argument 'hints' is invalid. Received ${options.hints}`
          ),
          { code: "ERR_INVALID_ARG_VALUE" }
        );
      }
      return new Promise((resolve, reject) =>
        __quenchDnsLookup(hostname, options, (error, address, family) =>
          error ? reject(error) : resolve({ address, family })
        )
      );
    }
  }
};
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  if (name === "dns" || name === "dns/promises") {
    return name === "dns" ? __quenchDns : __quenchDns.promises;
  }
  return __quenchOriginalRequireWithDns(specifier);
};
