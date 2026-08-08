globalThis.__quenchInternalFsBinding = {
  openFileHandle: (_path, _flags, _mode, _req, _context) => undefined,
  readdir: (path) => {
    const names = globalThis.__nodeFs.readdirSync(path);
    return [names, names.map(() => 1)];
  }
};
globalThis.__quenchInternalFallbackBinding = { fstat: () => undefined };
globalThis.__quenchDgramUdpFds = new Set();
globalThis.__quenchDgramUdpHandleInfo = new Map();
globalThis.__quenchDgramActiveFds = new Set();
let __quenchDgramNextHandleFd = 60000;
let __quenchDgramNextHandlePort = 45000;
globalThis.__quenchDgramUDPClass = class UDP {
  constructor() {
    this.fd = -1;
  }
  bind(address, port, _flags) {
    if (address === "localhost") return -99;
    this.fd = __quenchDgramNextHandleFd++;
    this._address = {
      address,
      port: port || __quenchDgramNextHandlePort++,
      family: address.includes(":") ? "IPv6" : "IPv4"
    };
    globalThis.__quenchDgramUdpFds.add(this.fd);
    globalThis.__quenchDgramUdpHandleInfo.set(this.fd, this._address);
    return 0;
  }
  bind6(address, port, flags) {
    return this.bind(address, port, flags);
  }
  getsockname(result) {
    Object.assign(result, this._address || {});
    return 0;
  }
  close() {
    globalThis.__quenchDgramUdpFds.delete(this.fd);
    this.fd = -1;
  }
};
globalThis.__quenchInternalBindingCore = (binding) => {
  if (binding === "buffer") {
    return {
      fill: (buffer, offset, end, value, encoding) => {
        if (
          !Number.isInteger(offset) ||
          !Number.isInteger(end) ||
          offset < 0 ||
          end < offset ||
          end > buffer.length ||
          (typeof value === "number" && (value < 0 || value > 255))
        ) {
          const error = new RangeError("value out of range");
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        return buffer.fill(value, offset, end, encoding);
      }
    };
  }
  if (binding === "fs") {
    return Object.assign(globalThis.__quenchInternalFsBinding, {
      fstat: (fd) => globalThis.__nodeFs.fstatSync(fd)
    });
  }
  if (binding === "udp_wrap") return { UDP: globalThis.__quenchDgramUDPClass };
  if (binding === "os") return globalThis.__quenchInternalOsBinding;
  if (binding === "debug") {
    return {
      getGenericUsageCount: (name) =>
        name.includes("Uninitialized")
          ? __nodeAllocatorCounts.uninitialized
          : __nodeAllocatorCounts.zeroFilled
    };
  }
};
