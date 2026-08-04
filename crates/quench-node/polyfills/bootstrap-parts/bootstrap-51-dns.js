const __quenchOriginalRequireWithDns = globalThis.require;
let __quenchDnsServers = ["127.0.0.1"];
const __quenchDnsLookup = (hostname, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  const address = hostname === "localhost" ? "127.0.0.1" : String(hostname);
  queueMicrotask(() => callback(null, address, 4));
};
class __quenchResolver {
  getServers() {
    return [...__quenchDnsServers];
  }
  setServers(servers) {
    __quenchDnsServers = [...servers];
  }
  lookup(hostname, options, callback) {
    __quenchDnsLookup(hostname, options, callback);
  }
}
const __quenchDns = {
  getServers: () => [...__quenchDnsServers],
  setServers: (servers) => {
    __quenchDnsServers = [...servers];
  },
  lookup: __quenchDnsLookup,
  Resolver: __quenchResolver,
  promises: {
    lookup: (hostname) =>
      new Promise((resolve, reject) =>
        __quenchDnsLookup(hostname, {}, (error, address, family) =>
          error ? reject(error) : resolve({ address, family })
        )
      )
  }
};
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  if (name === "dns" || name === "dns/promises")
    return name === "dns" ? __quenchDns : __quenchDns.promises;
  return __quenchOriginalRequireWithDns(specifier);
};
