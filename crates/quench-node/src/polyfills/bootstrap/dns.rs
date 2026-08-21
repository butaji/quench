//! Polyfill: `dns`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithDns = globalThis.require;
let __quenchDnsServers = ["127.0.0.1"];
let __quenchDnsDefaultResultOrder = "verbatim";
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
    const address = typeof server === "string" && net.isIP(server) === 0
      ? server.replace(/:\d+$/, "")
      : server;
    if (typeof server !== "string" || net.isIP(address) === 0) {
      throw Object.assign(new TypeError(`Invalid IP address: ${server}`), { code: "ERR_INVALID_IP_ADDRESS" });
    }
  }
  return normalized;
};
// Node's lookup validation has intentionally ordered diagnostics.
// eslint-disable-next-line max-lines-per-function, complexity
const __quenchDnsLookup = (hostname, options, callback) => {
  if (typeof hostname !== "string") {
    const received = hostname === null ? "null" : `${typeof hostname}`;
    throw Object.assign(new TypeError(`The "hostname" argument must be of type string. Received type ${received}`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  if (typeof options === "number") options = { family: options };
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (
    options !== undefined &&
    (options === null || typeof options !== "object")
  ) {
    throw Object.assign(
      new TypeError('The "options" argument must be of type object'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (typeof options?.family === "string") {
    throw Object.assign(new TypeError('The "family" option is invalid'), {
      code: options.family === "nodejs.org"
        ? "ERR_INVALID_ARG_TYPE"
        : "ERR_INVALID_ARG_VALUE",
    });
  }
  if (hostname.length === 0) {
    throw Object.assign(new TypeError("Invalid hostname"), {
      code: "ERR_INVALID_ARG_VALUE",
    });
  }
  if (hostname.includes("\0")) {
    throw Object.assign(
      new TypeError(
        "The argument 'hostname' must be a string without null bytes.",
      ),
      { code: "ERR_INVALID_ARG_VALUE" },
    );
  }
  if (
    options?.family !== undefined &&
    options.family !== 0 &&
    options.family !== 4 &&
    options.family !== 6
  ) {
    throw Object.assign(
      new TypeError(
        `The property 'options.family' must be one of: 0, 4, 6. Received ${options.family}`,
      ),
      { code: "ERR_INVALID_ARG_VALUE" },
    );
  }
  if (options?.hints !== undefined && options.hints % 2) {
    throw Object.assign(new TypeError(`The argument 'hints' is invalid. Received ${options.hints}`), { code: "ERR_INVALID_ARG_VALUE" });
  }
  queueMicrotask(() => {
    try {
      const addresses = globalThis.__quench_dns_lookup(hostname, 0);
      const family = options?.family === 6 ? 6 : options?.family === 4 ? 4 : 0;
      const filtered = addresses.filter((value) => {
        const detected = globalThis.require("net").isIP(value);
        return family === 0 || detected === family;
      });
      if (options?.all) {
        callback(
          null,
          filtered.map((value) => ({
            address: value,
            family: family || globalThis.require("net").isIP(value),
          })),
        );
        return;
      }
      const address = filtered[0];
      if (!address) {
        const error = new Error(`getaddrinfo ENOTFOUND ${hostname}`);
        error.code = "ENOTFOUND";
        error.syscall = "getaddrinfo";
        error.hostname = hostname;
        callback(error);
        return;
      }
      callback(null, address, globalThis.require("net").isIP(address));
    } catch (error) {
      error.code = "ENOTFOUND";
      error.syscall = "getaddrinfo";
      error.hostname = hostname;
      callback(error);
    }
  });
};
const __quenchDnsResolve = (hostname, rrtype, callback) => {
  if (typeof hostname !== "string") {
    throw Object.assign(new TypeError(`The "name" argument must be of type string. Received ${hostname}`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof rrtype === "function") {
    callback = rrtype;
    rrtype = "A";
  }
  if (typeof rrtype !== "string") {
    throw Object.assign(new TypeError('The "rrtype" argument must be of type string. Received an instance of Array'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  queueMicrotask(() => {
    if (rrtype !== "A" && rrtype !== "AAAA") {
      callback?.(null, []);
      return;
    }
    try {
      const addresses = globalThis.__quench_dns_lookup(hostname, 0)
        .filter((address) =>
          rrtype === "A"
            ? globalThis.require("net").isIPv4(address)
            : globalThis.require("net").isIPv6(address)
        );
      if (!addresses.length) {
        const error = new Error(`query${rrtype} ENOTFOUND ${hostname}`);
        error.code = "ENOTFOUND";
        error.syscall = `query${rrtype}`;
        error.hostname = hostname;
        callback?.(error);
        return;
      }
      callback?.(null, addresses);
    } catch (error) {
      error.code = "ENOTFOUND";
      error.syscall = `query${rrtype}`;
      error.hostname = hostname;
      callback?.(error);
    }
  });
};
const __quenchDnsLookupService = function __quenchDnsLookupService(
  address,
  port,
  callback,
) {
  if (typeof address !== "string" || arguments.length < 3) {
    throw Object.assign(new TypeError(`The "address", "port", and "callback" arguments must be specified`), { code: "ERR_MISSING_ARGS" });
  }
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (globalThis.require("net").isIP(address) === 0) {
    throw Object.assign(
      new TypeError(`The argument 'address' is invalid. Received '${address}'`),
      { code: "ERR_INVALID_ARG_VALUE" },
    );
  }
  if (
    typeof port !== "number" ||
    !Number.isInteger(port) ||
    port < 0 ||
    port > 65535
  ) {
    throw Object.assign(new RangeError("Port should be >= 0 and < 65536"), {
      code: "ERR_SOCKET_BAD_PORT",
    });
  }
  queueMicrotask(() => {
    try {
      callback(
        null,
        globalThis.__quench_dns_reverse(address),
        Number(port) === 22 ? "ssh" : "tcp",
      );
    } catch (error) {
      error.code = "ENOTFOUND";
      error.syscall = "getnameinfo";
      error.message = `getnameinfo ENOTFOUND ${address}`;
      callback(error);
    }
  });
};
const __quenchDnsReverse = (address, callback) => {
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (globalThis.require("net").isIP(address) === 0) {
    throw Object.assign(
      new TypeError(`The argument 'address' is invalid. Received '${address}'`),
      { code: "ERR_INVALID_ARG_VALUE" },
    );
  }
  queueMicrotask(() => {
    try {
      callback(null, [globalThis.__quench_dns_reverse(address)]);
    } catch (error) {
      error.code = "ENOTFOUND";
      error.syscall = "getnameinfo";
      error.hostname = address;
      callback(error);
    }
  });
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
const __quenchDnsValidateResultOrder = (order) => {
  if (!["verbatim", "ipv4first", "ipv6first"].includes(order)) {
    throw Object.assign(
      new TypeError(`The argument 'order' is invalid. Received '${order}'`),
      { code: "ERR_INVALID_ARG_VALUE" },
    );
  }
  return order;
};
class __quenchResolver {
  getServers() {
    return [...__quenchDnsServers];
  }
  setServers(servers) {
    __quenchDnsServers = __quenchDnsValidateServers(servers);
  }
  getDefaultResultOrder() {
    return __quenchDnsDefaultResultOrder;
  }
  setDefaultResultOrder(order) {
    __quenchDnsDefaultResultOrder = __quenchDnsValidateResultOrder(order);
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
  getDefaultResultOrder: () => __quenchDnsDefaultResultOrder,
  setDefaultResultOrder: (order) => {
    __quenchDnsDefaultResultOrder = __quenchDnsValidateResultOrder(order);
  },
  lookup: __quenchDnsLookup,
  resolve: __quenchDnsResolve,
  resolve4: (hostname, callback) => __quenchDnsResolve(hostname, "A", callback),
  resolve6: (hostname, callback) =>
    __quenchDnsResolve(hostname, "AAAA", callback),
  lookupService: __quenchDnsLookupService,
  reverse: __quenchDnsReverse,
  resolveMx: __quenchDnsResolveMx,
  Resolver: __quenchResolver,
  promises: {
    resolve: (hostname, rrtype = "A") => {
      if (typeof hostname !== "string") {
        throw Object.assign(
          new TypeError(
            `The "name" argument must be of type string. Received ${hostname}`,
          ),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      if (typeof rrtype !== "string") {
        throw Object.assign(
          new TypeError(
            'The "rrtype" argument must be of type string. Received an instance of Array',
          ),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      return new Promise((resolve, reject) =>
        __quenchDnsResolve(
          hostname,
          rrtype,
          (error, value) => error ? reject(error) : resolve(value),
        )
      );
    },
    resolve4: (hostname) =>
      new Promise((resolve, reject) =>
        __quenchDnsResolve(
          hostname,
          "A",
          (error, value) => error ? reject(error) : resolve(value),
        )
      ),
    resolve6: (hostname) =>
      new Promise((resolve, reject) =>
        __quenchDnsResolve(
          hostname,
          "AAAA",
          (error, value) => error ? reject(error) : resolve(value),
        )
      ),
    lookupService: (address, ...args) => {
      const port = args[0];
      if (typeof address !== "string" || args.length === 0) {
        throw Object.assign(
          new TypeError('The "address" and "port" arguments must be specified'),
          { code: "ERR_MISSING_ARGS" },
        );
      }
      if (globalThis.require("net").isIP(address) === 0) {
        throw Object.assign(
          new TypeError(
            `The argument 'address' is invalid. Received '${address}'`,
          ),
          { code: "ERR_INVALID_ARG_VALUE" },
        );
      }
      if (
        typeof port !== "number" ||
        !Number.isInteger(port) ||
        port < 0 ||
        port > 65535
      ) {
        throw Object.assign(new RangeError("Port should be >= 0 and < 65536"), {
          code: "ERR_SOCKET_BAD_PORT",
        });
      }
      return Promise.resolve().then(() => ({
        hostname: globalThis.__quench_dns_reverse(address),
        service: Number(port) === 22 ? "ssh" : "tcp",
      })).catch((error) => {
        error.code = "ENOTFOUND";
        error.syscall = "getnameinfo";
        error.message = `getnameinfo ENOTFOUND ${address}`;
        throw error;
      });
    },
    reverse: (address) =>
      new Promise((resolve, reject) =>
        __quenchDnsReverse(
          address,
          (error, value) => error ? reject(error) : resolve(value),
        )
      ),
    lookup: (hostname, options = {}) => {
      if (hostname && typeof hostname !== "string") {
        throw Object.assign(
          new TypeError(
            `The "hostname" argument must be of type string. Received type ${typeof hostname}`,
          ),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      if (!hostname) {
        return Promise.reject(
          Object.assign(new TypeError("Invalid hostname"), {
            code: "ERR_INVALID_ARG_VALUE",
          }),
        );
      }
      if (hostname.includes("\0")) {
        throw Object.assign(
          new TypeError(
            "The argument 'hostname' must be a string without null bytes.",
          ),
          { code: "ERR_INVALID_ARG_VALUE" },
        );
      }
      if (
        options.family !== undefined &&
        options.family !== 0 &&
        options.family !== 4 &&
        options.family !== 6
      ) {
        throw Object.assign(
          new TypeError(
            `The property 'options.family' must be one of: 0, 4, 6. Received ${options.family}`,
          ),
          { code: "ERR_INVALID_ARG_VALUE" },
        );
      }
      if (options.hints !== undefined && options.hints % 2) {
        throw Object.assign(
          new TypeError(
            `The argument 'hints' is invalid. Received ${options.hints}`,
          ),
          { code: "ERR_INVALID_ARG_VALUE" },
        );
      }
      return new Promise((resolve, reject) =>
        __quenchDnsLookup(
          hostname,
          options,
          (error, address, family) => {
            if (error) return reject(error);
            if (options.all) return resolve(address);
            resolve({ address, family });
          },
        )
      );
    },
  },
};
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  if (name === "dns" || name === "dns/promises") {
    return name === "dns" ? __quenchDns : __quenchDns.promises;
  }
  return __quenchOriginalRequireWithDns(specifier);
};
"#);
