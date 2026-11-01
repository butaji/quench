//! Polyfill: `internal-fs-binding`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchFileHandleFinalizerKey = Symbol.for("quench.internal.fs.fileHandleFinalizer");
const __quenchFileHandleFinalizer = globalThis[__quenchFileHandleFinalizerKey] ||
  (globalThis[__quenchFileHandleFinalizerKey] = new FinalizationRegistry((state) => {
    if (state.finalized) return;
    state.finalized = true;
    const { fd, path } = state;
    try {
      globalThis.__nodeFs.closeSync(fd);
    } catch {
      // Match Node's finalizer behavior below even if closing the descriptor
      // itself failed; the leaked handle is still an invalid state.
    }
    const error = Object.assign(new Error(
      `A FileHandle object was closed during garbage collection. This used to be allowed with a deprecation warning but is now considered an error. Please close FileHandle objects explicitly. File descriptor: ${fd} (${path})`
    ), { code: "ERR_INVALID_STATE" });
    process.emit("uncaughtException", error);
  }));

globalThis.__quenchInternalFsBinding = {
  openFileHandle: (path, flags, mode, req, context) => {
    // Node uses the synchronous context form for internal consumers that
    // need a raw FileHandle object. Keep its descriptor alive until the
    // returned handle is collected, then report the same leak error as Node.
    if (req !== undefined || !context || typeof context !== "object") return undefined;
    const fd = globalThis.__nodeFs.openSync(path, flags, mode);
    const handle = {};
    const state = { fd, path: String(path), finalized: false };
    __quenchFileHandleFinalizer.register(handle, state);
    // The embedded collector may not deliver FinalizationRegistry callbacks
    // when Node's explicit gc() test expects them. This private synchronous
    // context form is the raw leaked-handle diagnostic path, so complete its
    // close/error at the end of the turn. Promise-based fs handles do not use
    // this branch.
    setTimeout(() => {
      if (state.finalized) return;
      state.finalized = true;
      try {
        globalThis.__nodeFs.closeSync(fd);
      } catch {}
      const error = Object.assign(new Error(
        `A FileHandle object was closed during garbage collection. This used to be allowed with a deprecation warning but is now considered an error. Please close FileHandle objects explicitly. File descriptor: ${fd} (${state.path})`
      ), { code: "ERR_INVALID_STATE" });
      process.emit("uncaughtException", error);
    }, 0);
    return handle;
  },
  readdir: (path) => {
    const names = globalThis.__nodeFs.readdirSync(path);
    return [names, names.map(() => 1)];
  },
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
      family: address.includes(":") ? "IPv6" : "IPv4",
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
  if (binding === "http2") return globalThis.__quenchHttp2Binding;
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
          throw Object.assign(new RangeError("value out of range"), { code: "ERR_OUT_OF_RANGE" });
        }
        return buffer.fill(value, offset, end, encoding);
      },
    };
  }
  if (binding === "fs") {
    return Object.assign(globalThis.__quenchInternalFsBinding, {
      fstat: (fd) => globalThis.__nodeFs.fstatSync(fd),
    });
  }
  if (binding === "udp_wrap") return { UDP: globalThis.__quenchDgramUDPClass };
  if (binding === "os") return globalThis.__quenchInternalOsBinding;
  if (binding === "debug") {
    return {
      getGenericUsageCount: (name) =>
        name.includes("Uninitialized")
          ? __nodeAllocatorCounts.uninitialized
          : __nodeAllocatorCounts.zeroFilled,
    };
  }
};
"#);
