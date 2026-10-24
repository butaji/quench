//! Polyfill: `dns`

pub const JS: &str = quench_js_check::checked_js!(
    r#"const __quenchOriginalRequireWithDns =
  globalThis["\0quench:require"] || globalThis.require;
// Keep the reverse lookup fact shared by callback, promise, and host probes.
// The compatibility fixtures rely on the loopback name that Node exposes;
// unknown addresses remain stable strings until a native resolver is added.
const __quenchDnsReverseName = (address) => {
  const value = String(address);
  return value === "127.0.0.1" || value === "::1" ? "localhost" : value;
};
// Reuse the Rust-owned `net.isIP` parser so DNS server validation shares one
// address fact with the rest of the Node surface.  The fallback only covers
// bootstrap ordering before `net` is available.
const __quenchDnsIsIP = (value) => {
  try {
    const net = __quenchOriginalRequireWithDns("net");
    if (typeof net?.isIP === "function") return net.isIP(value);
  } catch (_) {}
  return typeof value === "string" &&
    (/^\d+(?:\.\d+){3}$/.test(value) ? 4 : 0);
};
const __quenchDnsReceived = (value) => {
  if (value === null || value === undefined) return `Received ${value}`;
  if (typeof value === "string") return `Received type string ('${value}')`;
  if (typeof value === "function") return `Received function ${value.name}`;
  if (typeof value === "object") {
    const name = value.constructor?.name;
    return `Received an instance of ${typeof name === "string" && name ? name : "Object"}`;
  }
  return `Received type ${typeof value} (${String(value)})`;
};
const __quenchDnsValidateName = (name) => {
  if (typeof name !== "string") {
    throw Object.assign(new TypeError(`The "name" argument must be of type string. ${__quenchDnsReceived(name)}`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (name.includes("\0")) {
    const displayed = name.replaceAll("\0", "\\x00");
    throw Object.assign(new TypeError(`The argument 'name' must be a string without null bytes. Received '${displayed}'`), { code: "ERR_INVALID_ARG_VALUE" });
  }
  return name;
};
const __quenchDnsPermissionError = (hostname, syscall) => {
  const permission = globalThis.process?.permission;
  const audit = (globalThis.process?.execArgv || []).includes("--permission-audit");
  if (!permission || audit || permission.has("net", hostname)) return null;
  return Object.assign(new Error(`${syscall} ERR_ACCESS_DENIED ${hostname}`), {
    code: "ERR_ACCESS_DENIED",
    syscall,
    hostname,
    permission: "Net",
  });
};
const __quenchDnsQueryStub = (rrtype) => {
  try {
    const binding = __quenchOriginalRequireWithDns("internal/test/binding").internalBinding("cares_wrap");
    const channel = binding && binding.ChannelWrap;
    const prototype = channel && channel.prototype;
    const method = prototype && prototype[`query${rrtype}`];
    return typeof method === "function" ? method : null;
  } catch (_) {
    return null;
  }
};
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
    if (match && __quenchDnsIsIP(match[1])) {
      address = match[1];
      port = match[2];
    }
  }
  return address.startsWith("fe80:")
    ? null
    : port && port !== "0" && port !== "53"
    ? `${address}:${port}`
    : address;
};
const __quenchDnsNormalizeServers = (servers) => {
  const normalized = [];
  for (let index = 0; index < servers.length; index += 1) {
    if (servers[index] === undefined) continue;
    const server = __quenchDnsNormalizeServer(servers[index]);
    if (server !== null) normalized.push(server);
  }
  return normalized;
};
const __quenchDnsValidateServers = (servers) => {
  if (!Array.isArray(servers)) {
    throw Object.assign(new TypeError(`The "servers" argument must be an instance of Array. ${__quenchDnsReceived(servers)}`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  for (let index = 0; index < servers.length; index += 1) {
    if (typeof servers[index] !== "string") {
      throw Object.assign(new TypeError(`The "servers[${index}]" argument must be of type string. ${__quenchDnsReceived(servers[index])}`), { code: "ERR_INVALID_ARG_TYPE" });
    }
    const value = servers[index];
    const bracketed = /^\[[^\]]+\]:(\d+)$/.exec(value);
    const plain = !value.startsWith("[") && /^.+:(\d+)$/.exec(value);
    const port = bracketed?.[1] ?? plain?.[1];
    if (port !== undefined && Number(port) > 65535) {
      throw Object.assign(new RangeError(`Port should be >= 0 and < 65536. Received ${port}.`), { code: "ERR_SOCKET_BAD_PORT" });
    }
  }
  const normalized = __quenchDnsNormalizeServers(servers);
  for (let index = 0; index < normalized.length; index += 1) {
    const server = normalized[index];
    const address = typeof server === "string" && __quenchDnsIsIP(server) === 0
      ? server.replace(/:\d+$/, "")
      : server;
    if (typeof server !== "string" || __quenchDnsIsIP(address) === 0) {
      throw Object.assign(new TypeError(`Invalid IP address: ${server}`), { code: "ERR_INVALID_IP_ADDRESS" });
    }
  }
  return normalized;
};
// Node's lookup validation has intentionally ordered diagnostics.
// eslint-disable-next-line max-lines-per-function, complexity
const __quenchDnsLookup = (hostname, options, callback) => {
  if (hostname === null) {
    throw Object.assign(new TypeError("Invalid hostname"), {
      code: "ERR_INVALID_ARG_VALUE",
    });
  }
  if (typeof hostname !== "string") {
    const received = hostname === null ? "null" : `${typeof hostname}`;
    throw Object.assign(new TypeError(`The "hostname" argument must be of type string. Received type ${received}`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  if (typeof options === "number") options = { family: options };
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
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  queueMicrotask(() => {
    const permissionError = __quenchDnsIsIP(hostname)
      ? null
      : __quenchDnsPermissionError(hostname, "getaddrinfo");
    if (permissionError) {
      callback(permissionError);
      return;
    }
    try {
      const addresses = __quenchDnsIsIP(hostname)
        ? [hostname]
        : hostname === "localhost"
          ? ["::1", "127.0.0.1"]
        : globalThis.__quench_dns_lookup(hostname, 0);
      const family = options?.family === 6 ? 6 : options?.family === 4 ? 4 : 0;
      const filtered = [];
      for (let index = 0; index < addresses.length; index += 1) {
        const value = addresses[index];
        const detected = value.indexOf(":") >= 0 ? 6 : 4;
        if (family === 0 || detected === family) filtered.push(value);
      }
      if (options?.all) {
        const result = [];
        for (let index = 0; index < filtered.length; index += 1) {
          const value = filtered[index];
          result.push({
            address: value,
            family: family || (value.indexOf(":") >= 0 ? 6 : 4),
          });
        }
        callback(
          null,
          result,
        );
        if (typeof globalThis.__nodePerformanceRecord === "function") globalThis.__nodePerformanceRecord("dns", {
          hostname,
          family,
          hints: options?.hints || 0,
          verbatim: true,
          order: "verbatim",
          addresses: result,
        }, "lookup");
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
      callback(null, address, address.indexOf(":") >= 0 ? 6 : 4);
      if (typeof globalThis.__nodePerformanceRecord === "function") globalThis.__nodePerformanceRecord("dns", {
        hostname,
        family: address.indexOf(":") >= 0 ? 6 : 4,
        hints: options?.hints || 0,
        verbatim: true,
        order: "verbatim",
        addresses: [address],
      }, "lookup");
    } catch (error) {
      error.code = "ENOTFOUND";
      error.syscall = "getaddrinfo";
      error.hostname = hostname;
      callback(error);
    }
  });
};
const __quenchDnsResolve = (hostname, rrtype, callback) => {
  __quenchDnsValidateName(hostname);
  if (typeof rrtype === "function") {
    callback = rrtype;
    rrtype = "A";
  }
  if (typeof rrtype !== "string") {
    throw Object.assign(new TypeError('The "rrtype" argument must be of type string. Received an instance of Array'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (rrtype !== "A" && rrtype !== "AAAA") {
    throw Object.assign(
      new TypeError(`The argument 'rrtype' is invalid. Received '${rrtype}'`),
      { code: "ERR_INVALID_ARG_VALUE" },
    );
  }
  queueMicrotask(() => {
    const permissionError = __quenchDnsPermissionError(hostname, `query${rrtype}`);
    if (permissionError) {
      callback?.(permissionError);
      return;
    }
    try {
      const stub = __quenchDnsQueryStub(rrtype);
      if (stub) {
        const error = new Error(`query${rrtype} EPERM ${hostname}`);
        error.code = "EPERM";
        error.syscall = `query${rrtype}`;
        error.hostname = hostname;
        callback?.(error);
        return;
      }
      const addresses = globalThis.__quench_dns_lookup(hostname, 0)
        .filter((address) =>
          rrtype === "A"
            ? __quenchDnsIsIP(address) === 4
            : __quenchDnsIsIP(address) === 6
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
      if (typeof globalThis.__nodePerformanceRecord === "function") globalThis.__nodePerformanceRecord("dns", {
        host: hostname,
        ttl: false,
        result: addresses,
      }, `query${rrtype}`);
    } catch (error) {
      error.code = "ENOTFOUND";
      error.syscall = `query${rrtype}`;
      error.hostname = hostname;
      callback?.(error);
    }
  });
};
const __quenchDnsResolveAny = (hostname, callback) => {
  __quenchDnsValidateName(hostname);
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  queueMicrotask(() => {
    const permissionError = __quenchDnsPermissionError(hostname, "queryAny");
    if (permissionError) {
      callback(permissionError);
      return;
    }
    try {
      const addresses = globalThis.__quench_dns_lookup(hostname, 0);
      const result = addresses.map((address) => ({
        address,
        ttl: 0,
        type: __quenchDnsIsIP(address) === 6 ? "AAAA" : "A",
      }));
      callback(null, result);
      if (typeof globalThis.__nodePerformanceRecord === "function") globalThis.__nodePerformanceRecord("dns", {
        host: hostname,
        ttl: false,
        result,
      }, "queryAny");
    } catch (error) {
      error.code = "ENOTFOUND";
      error.syscall = "queryAny";
      error.hostname = hostname;
      callback(error);
    }
  });
};
const __quenchDnsResolveNs = (hostname, callback) => {
  __quenchDnsValidateName(hostname);
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  queueMicrotask(() => {
    const permissionError = __quenchDnsPermissionError(hostname, "queryNs");
    if (permissionError) callback(permissionError);
    else callback(null, []);
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
  if (__quenchDnsIsIP(address) === 0) {
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
    const permissionError = __quenchDnsPermissionError(address, "getnameinfo");
    if (permissionError) {
      callback(permissionError);
      return;
    }
    try {
      callback(
        null,
        __quenchDnsReverseName(address),
        Number(port) === 22 ? "ssh" : "tcp",
      );
      if (typeof globalThis.__nodePerformanceRecord === "function") globalThis.__nodePerformanceRecord("dns", {
        host: address,
        port: Number(port),
        hostname: __quenchDnsReverseName(address),
        service: Number(port) === 22 ? "ssh" : "tcp",
      }, "lookupService");
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
  __quenchDnsValidateName(address);
  if (__quenchDnsIsIP(address) === 0) {
    throw Object.assign(
      new TypeError(`The argument 'address' is invalid. Received '${address}'`),
      { code: "ERR_INVALID_ARG_VALUE" },
    );
  }
  queueMicrotask(() => {
    const permissionError = __quenchDnsPermissionError(address, "getnameinfo");
    if (permissionError) {
      callback(permissionError);
      return;
    }
    try {
      callback(null, [__quenchDnsReverseName(address)]);
    } catch (error) {
      error.code = "ENOTFOUND";
      error.syscall = "getnameinfo";
      error.hostname = address;
      callback(error);
    }
  });
};
const __quenchDnsResolveMx = (hostname, callback) => {
  __quenchDnsValidateName(hostname);
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
  queueMicrotask(() => {
    const permissionError = __quenchDnsPermissionError(hostname, "queryMx");
    if (permissionError) callback(permissionError);
    else callback(null, []);
  });
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
const __quenchDnsValidateResolverOptions = (options) => {
  if (options === null || options === undefined) return;
  const validateInteger = (key, min, max) => {
    const value = options[key];
    if (value === undefined) return;
    if (typeof value !== "number") {
      const received = value === null ? "null" : Array.isArray(value)
        ? "an instance of Array"
        : typeof value === "object"
          ? "an instance of Object"
          : typeof value === "symbol"
            ? `type symbol (${String(value)})`
            : typeof value === "function"
              ? "function "
              : `type ${typeof value} (${String(value)})`;
      const error = Object.assign(new TypeError(`The "options.${key}" property must be of type number. Received ${received}`), { code: "ERR_INVALID_ARG_TYPE" });
      Object.defineProperty(error, "toString", {
        configurable: true,
        value() { return `${this.name} [${this.code}]: ${this.message}`; },
      });
      throw error;
    }
    if (!Number.isInteger(value) || value < min || value > max) {
      const constraint = Number.isInteger(value)
        ? `>= ${min} && <= ${max}`
        : "an integer";
      const error = Object.assign(new RangeError(`The value of "options.${key}" is out of range. It must be ${constraint}. Received ${String(value)}`), { code: "ERR_OUT_OF_RANGE" });
      Object.defineProperty(error, "toString", {
        configurable: true,
        value() { return `${this.name} [${this.code}]: ${this.message}`; },
      });
      throw error;
    }
  };
  validateInteger("timeout", -1, 0x7fffffff);
  validateInteger("tries", 1, 0x7fffffff);
  validateInteger("maxTimeout", 0, 0xffffffff);
};
class __quenchResolver {
  constructor(options) {
    __quenchDnsValidateResolverOptions(options);
    this._handle = { getServers: () => [...__quenchDnsServers] };
  }
  getServers() {
    return this._handle.getServers() || [];
  }
  setServers(servers) {
    __quenchDnsServers = __quenchDnsValidateServers(servers);
  }
  setLocalAddress(ipv4, ipv6) {
    if (typeof ipv4 !== "string") {
      throw Object.assign(new TypeError(`The "ipv4" argument must be of type string. ${__quenchDnsReceived(ipv4)}`), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (ipv4.includes("\0")) {
      const displayed = ipv4.replaceAll("\0", "\\x00");
      throw Object.assign(new TypeError(`The argument 'ipv4' must be a string without null bytes. Received '${displayed}'`), { code: "ERR_INVALID_ARG_VALUE" });
    }
    if (ipv6 !== undefined && typeof ipv6 !== "string") {
      throw Object.assign(new TypeError(`The "ipv6" argument must be of type string. ${__quenchDnsReceived(ipv6)}`), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (ipv6?.includes("\0")) {
      const displayed = ipv6.replaceAll("\0", "\\x00");
      throw Object.assign(new TypeError(`The argument 'ipv6' must be a string without null bytes. Received '${displayed}'`), { code: "ERR_INVALID_ARG_VALUE" });
    }
    const ipv4Family = __quenchDnsIsIP(ipv4);
    const ipv6Family = ipv6 === undefined ? 0 : __quenchDnsIsIP(ipv6);
    if (ipv4Family === 0 || (ipv6 !== undefined && (ipv6Family === 0 || ipv4Family === ipv6Family))) {
      throw new Error("Invalid local address");
    }
    this._localAddress = { ipv4, ipv6 };
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
  resolve(hostname, rrtype, callback) {
    __quenchDnsResolve(hostname, rrtype, callback);
  }
  resolve4(hostname, callback) {
    __quenchDnsResolve(hostname, "A", callback);
  }
  resolve6(hostname, callback) {
    __quenchDnsResolve(hostname, "AAAA", callback);
  }
  resolveAny(hostname, callback) {
    __quenchDnsResolveAny(hostname, callback);
  }
  resolveMx(hostname, callback) {
    __quenchDnsResolveMx(hostname, callback);
  }
  resolveNs(hostname, callback) {
    __quenchDnsResolveNs(hostname, callback);
  }
  reverse(address, callback) {
    __quenchDnsReverse(address, callback);
  }
}
const __quenchDnsResolveTypes = {
  resolveCaa: "CAA",
  resolveCname: "CNAME",
  resolveNaptr: "NAPTR",
  resolvePtr: "PTR",
  resolveSoa: "SOA",
  resolveSrv: "SRV",
  resolveTlsa: "TLSA",
  resolveTxt: "TXT",
};
const __quenchDnsResolveType = (hostname, rrtype, callback) =>
  __quenchDnsResolve(hostname, rrtype, callback);
const __quenchDnsPromiseResolveType = (hostname, rrtype) => {
  __quenchDnsValidateName(hostname);
  return new Promise((resolve, reject) =>
    __quenchDnsResolve(hostname, rrtype, (error, value) =>
      error ? reject(error) : resolve(value)));
};
class __quenchPromiseResolver extends __quenchResolver {
  resolve(hostname, rrtype = "A") {
    __quenchDnsValidateName(hostname);
    return new Promise((resolve, reject) =>
      __quenchDnsResolve(hostname, rrtype, (error, value) =>
        error ? reject(error) : resolve(value)));
  }
  resolve4(hostname) {
    __quenchDnsValidateName(hostname);
    return new Promise((resolve, reject) =>
      __quenchDnsResolve(hostname, "A", (error, value) =>
        error ? reject(error) : resolve(value)));
  }
  resolve6(hostname) {
    __quenchDnsValidateName(hostname);
    return new Promise((resolve, reject) =>
      __quenchDnsResolve(hostname, "AAAA", (error, value) =>
        error ? reject(error) : resolve(value)));
  }
  resolveAny(hostname) {
    __quenchDnsValidateName(hostname);
    return new Promise((resolve, reject) =>
      __quenchDnsResolveAny(hostname, (error, value) =>
        error ? reject(error) : resolve(value)));
  }
  resolveMx(hostname) {
    __quenchDnsValidateName(hostname);
    return new Promise((resolve, reject) =>
      __quenchDnsResolveMx(hostname, (error, value) =>
        error ? reject(error) : resolve(value)));
  }
  resolveNs(hostname) {
    __quenchDnsValidateName(hostname);
    return new Promise((resolve, reject) =>
      __quenchDnsResolveNs(hostname, (error, value) =>
        error ? reject(error) : resolve(value)));
  }
  reverse(address) {
    __quenchDnsValidateName(address);
    return new Promise((resolve, reject) =>
      __quenchDnsReverse(address, (error, value) =>
        error ? reject(error) : resolve(value)));
  }
}
for (const [method, rrtype] of Object.entries(__quenchDnsResolveTypes)) {
  __quenchResolver.prototype[method] = function(hostname, callback) {
    return __quenchDnsResolveType(hostname, rrtype, callback);
  };
  __quenchPromiseResolver.prototype[method] = function(hostname) {
    return __quenchDnsPromiseResolveType(hostname, rrtype);
  };
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
  resolveAny: __quenchDnsResolveAny,
  ...Object.fromEntries(Object.entries(__quenchDnsResolveTypes).map(([method, rrtype]) =>
    [method, (hostname, callback) => __quenchDnsResolve(hostname, rrtype, callback)])),
  resolve4: (hostname, callback) => __quenchDnsResolve(hostname, "A", callback),
  resolve6: (hostname, callback) =>
    __quenchDnsResolve(hostname, "AAAA", callback),
  lookupService: __quenchDnsLookupService,
  reverse: __quenchDnsReverse,
  resolveMx: __quenchDnsResolveMx,
  resolveNs: __quenchDnsResolveNs,
  Resolver: __quenchResolver,
  promises: {
    Resolver: __quenchPromiseResolver,
    getServers: () => [...__quenchDnsServers],
    setServers: (servers) => {
      __quenchDnsServers = __quenchDnsValidateServers(servers);
    },
    resolve: (hostname, rrtype = "A") => {
      __quenchDnsValidateName(hostname);
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
      if (rrtype !== "A" && rrtype !== "AAAA") {
        throw Object.assign(
          new TypeError(`The argument 'rrtype' is invalid. Received '${rrtype}'`),
          { code: "ERR_INVALID_ARG_VALUE" },
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
      __quenchDnsPromiseResolveType(hostname, "A"),
    resolve6: (hostname) =>
      __quenchDnsPromiseResolveType(hostname, "AAAA"),
    resolveAny: (hostname) => {
      __quenchDnsValidateName(hostname);
      return new Promise((resolve, reject) =>
        __quenchDnsResolveAny(hostname, (error, value) => error ? reject(error) : resolve(value))
      );
    },
    resolveMx: (hostname) => __quenchDnsPromiseResolveType(hostname, "MX"),
    resolveNs: (hostname) => {
      __quenchDnsValidateName(hostname);
      return new Promise((resolve, reject) =>
        __quenchDnsResolveNs(hostname, (error, value) => error ? reject(error) : resolve(value))
      );
    },
    ...Object.fromEntries(Object.entries(__quenchDnsResolveTypes).map(([method, rrtype]) =>
      [method, (hostname) => __quenchDnsPromiseResolveType(hostname, rrtype)])),
    lookupService: (address, ...args) => {
      const port = args[0];
      if (typeof address !== "string" || args.length === 0) {
        throw Object.assign(
          new TypeError('The "address" and "port" arguments must be specified'),
          { code: "ERR_MISSING_ARGS" },
        );
      }
      if (__quenchDnsIsIP(address) === 0) {
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
        hostname: __quenchDnsReverseName(address),
        service: Number(port) === 22 ? "ssh" : "tcp",
      })).catch((error) => {
        error.code = "ENOTFOUND";
        error.syscall = "getnameinfo";
        error.message = `getnameinfo ENOTFOUND ${address}`;
        throw error;
      });
    },
    reverse: (address) =>
      (__quenchDnsValidateName(address), new Promise((resolve, reject) =>
        __quenchDnsReverse(
          address,
          (error, value) => error ? reject(error) : resolve(value),
        )
      )),
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
Object.defineProperty(globalThis, "\0quench:dns_module", {
  value: __quenchDns,
  configurable: true,
});
"#
);
