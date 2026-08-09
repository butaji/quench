const __quenchNetModule = {
  isIP,
  isIPv4,
  isIPv6,
  getDefaultAutoSelectFamily: () => false,
  setDefaultAutoSelectFamily: () => undefined,
  getDefaultAutoSelectFamilyAttemptTimeout: () => 250,
  Socket: __quenchNetSocket,
  createConnection: (options, callback) => {
    globalThis.__quenchValidateConnectionOptions(options);
    const socket = new __quenchNetModule.Socket();
    socket._handle = { setKeepAlive: () => {} };
    socket.connect(options, callback);
    if (__quenchNativeTransportRequested(options)) return socket;
    return socket;
  },
  setDefaultAutoSelectFamilyAttemptTimeout: () => undefined,
  BlockList: class BlockList {
    [Symbol.toStringTag] = "BlockList";
    constructor() {
      this._v4 = new Map();
      this._v6 = new Map();
      this._v4Ranges = [];
      this._v6Ranges = [];
      this._v4Subnets = [];
      this._v6Subnets = [];
      this._rules = [];
    }
    get rules() {
      return this._rules.slice().reverse();
    }
    [Symbol.for("nodejs.util.inspect.custom")](options) {
      return this;
    }
    _checkType(type) {
      if (typeof type !== "string") {
        const e = new TypeError("Invalid type [ERR_INVALID_ARG_TYPE]");
        e.code = "ERR_INVALID_ARG_TYPE";
        throw e;
      }
      const lower = type.toLowerCase();
      if (lower !== "ipv4" && lower !== "ipv6") {
        const e = new TypeError(
          `Invalid type '${type}' [ERR_INVALID_ARG_VALUE]`,
        );
        e.code = "ERR_INVALID_ARG_VALUE";
        throw e;
      }
      return lower;
    }
    addAddress(address, type) {
      const normalized = normalizeAddress(address);
      const str = normalized.value;
      const explicit = type !== undefined || normalized.explicit;
      const resolvedType = resolveAddressType(
        str,
        type,
        (value) => this._checkType(value),
      );
      if (resolvedType === "ipv4") {
        const existing = this._v4.get(str);
        this._v4.set(str, {
          explicit: (existing && existing.explicit) || explicit,
        });
        this._rules.push(`Address: IPv4 ${str}`);
      } else {
        const existing = this._v6.get(str);
        this._v6.set(str, {
          explicit: (existing && existing.explicit) || explicit,
        });
        this._rules.push(`Address: IPv6 ${str}`);
      }
    }
    addRange(start, end, type) {
      start = normalizeRangeEndpoint(start, "start");
      end = normalizeRangeEndpoint(end, "end");
      let resolvedType = type;
      if (resolvedType === undefined) {
        resolvedType = isIPv4(start) ? "ipv4" : "ipv6";
      } else {
        resolvedType = this._checkType(resolvedType);
      }
      if (resolvedType === "ipv4") {
        if (compareV4(start, end) > 0) {
          const e = new TypeError(
            'The value of "start" must be lower than "end" [ERR_INVALID_ARG_VALUE]',
          );
          e.code = "ERR_INVALID_ARG_VALUE";
          throw e;
        }
        this._v4Ranges.push([start, end]);
        this._rules.push(`Range: IPv4 ${start}-${end}`);
      } else {
        if (compareV6(start, end) > 0) {
          const e = new TypeError(
            'The value of "start" must be lower than "end" [ERR_INVALID_ARG_VALUE]',
          );
          e.code = "ERR_INVALID_ARG_VALUE";
          throw e;
        }
        this._v6Ranges.push([start, end]);
        this._rules.push(`Range: IPv6 ${start}-${end}`);
      }
    }
    addSubnet(net, prefix, type) {
      net = normalizeRangeEndpoint(net, "net");
      if (typeof prefix !== "number") {
        const e = new TypeError("Invalid prefix [ERR_INVALID_ARG_TYPE]");
        e.code = "ERR_INVALID_ARG_TYPE";
        throw e;
      }
      const resolvedType = resolveAddressType(
        net,
        type,
        (value) => this._checkType(value),
      );
      const maxPrefix = resolvedType === "ipv4" ? 32 : 128;
      if (!Number.isFinite(prefix) || prefix < 0 || prefix > maxPrefix) {
        const e = new TypeError(
          `Prefix must be between 0 and ${maxPrefix} [ERR_OUT_OF_RANGE]`,
        );
        e.code = "ERR_OUT_OF_RANGE";
        throw e;
      }
      if (resolvedType === "ipv4") {
        this._v4Subnets.push([net, prefix]);
        this._rules.push(`Subnet: IPv4 ${net}/${prefix}`);
      } else {
        this._v6Subnets.push([net, prefix]);
        this._rules.push(`Subnet: IPv6 ${net}/${prefix}`);
      }
    }
    check(address, type) {
      const { str, resolvedType, explicitType, inputKind } =
        resolveBlockListCheck(address, type, (value) => this._checkType(value));
      if (resolvedType === null) return false;
      return resolvedType === "ipv4"
        ? checkBlockListV4(this, str, explicitType, inputKind)
        : checkBlockListV6(this, str, explicitType, inputKind);
    }
  },
  SocketAddress: class SocketAddress {
    constructor(input) {
      this.address = input && input.address ? String(input.address) : "";
      this.family = input && input.family !== undefined
        ? input.family
        : undefined;
      this.flowlabel = (input && input.flowlabel) || 0;
      this.port = (input && input.port) || 0;
    }
  },
  BoundSocket: class BoundSocket {
    constructor(options = {}) {
      if (!options || typeof options !== "object" || Array.isArray(options)) {
        const error = new TypeError("options must be an object");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const host = options.host ?? (options.ipv6Only ? "::" : "0.0.0.0");
      if (options.path !== undefined) {
        if (typeof options.path !== "string") {
          const error = new TypeError("path must be a string");
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (options.path.startsWith("\0") && process.platform !== "linux") {
          const error = new TypeError("abstract socket paths are Linux-only");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (
          options.host !== undefined ||
          options.port !== undefined ||
          options.ipv6Only !== undefined ||
          options.reusePort !== undefined
        ) {
          const error = new TypeError(
            "path cannot be combined with TCP options",
          );
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (options.path.includes("nope/")) {
          const error = new Error("No such file or directory");
          error.code = "EACCES";
          error.syscall = "bind";
          throw error;
        }
        if (options.path.length > 1023) {
          const error = new Error("path too long");
          error.code = "EINVAL";
          error.syscall = "bind";
          throw error;
        }
        if (__quenchBoundPaths.has(options.path)) {
          const error = new Error("address already in use");
          error.code = "EADDRINUSE";
          error.syscall = "bind";
          throw error;
        }
        __quenchBoundPaths.add(options.path);
        this._path = options.path;
        this._host = options.path;
        this._port = 0;
        this._closed = false;
        this._adopted = false;
        return;
      }
      if (typeof host !== "string") {
        const error = new TypeError("host must be a string");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (!isIPv4(host) && !isIPv6(host) && host !== "0.0.0.0") {
        const error = new TypeError("host must be an IPv4 address");
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      if (host.startsWith("192.0.2.")) {
        const error = new Error("Cannot assign requested address");
        error.code = "EADDRNOTAVAIL";
        error.syscall = "bind";
        throw error;
      }
      const requested = Number(options.port ?? 0);
      if (requested > 0 && requested < 1024 && process.getuid?.() !== 0) {
        const error = new Error("permission denied");
        error.code = "EACCES";
        error.syscall = "bind";
        throw error;
      }
      this._port = requested || __quenchNextEphemeralPort++;
      if (
        (!options.reusePort && __quenchBoundPorts.has(this._port)) ||
        [...__quenchNetServers].some(
          (server) => server.listening && server.address().port === this._port,
        )
      ) {
        const error = new Error("address already in use");
        error.code = "EADDRINUSE";
        error.syscall = "bind";
        throw error;
      }
      __quenchBoundPorts.add(this._port);
      this._host = host;
      this._reusePort = options.reusePort === true;
      this._family = isIPv6(host) ? "IPv6" : "IPv4";
      this._closed = false;
      this._adopted = false;
    }
    _assertOpen() {
      if (this._adopted) {
        const error = new Error("BoundSocket handle was adopted");
        error.code = "ERR_SOCKET_HANDLE_ADOPTED";
        throw error;
      }
      if (this._closed) {
        const error = new Error("BoundSocket is closed");
        error.code = "ERR_SOCKET_CLOSED";
        throw error;
      }
    }
    address() {
      this._assertOpen();
      if (this._path !== undefined) return this._path;
      return { address: this._host, family: this._family, port: this._port };
    }
    fd() {
      this._assertOpen();
      return this._port;
    }
    close() {
      this._assertOpen();
      this._closed = true;
      if (this._path !== undefined) __quenchBoundPaths.delete(this._path);
      else __quenchBoundPorts.delete(this._port);
    }
    get isPipe() {
      return this._path !== undefined;
    }
  },
  createServer: (options, handler) => {
    if (typeof options === "function") {
      handler = options;
      options = {};
    }
    const server = new globalThis.__nodeEventEmitter();
    server.listening = false;
    server._connections = new Set();
    server._closeRequested = false;
    server._nativeId = 0;
    server._nativeTransport = false;
    server._port = 0;
    server._path = undefined;
    server._handle = { close: () => {} };
    server.keepAlive = options?.keepAlive;
    server.keepAliveInitialDelay = options?.keepAliveInitialDelay;
    server._allowHalfOpen = options?.allowHalfOpen === true;
    server.address = () => {
      if (!server.listening) return null;
      if (server._path !== undefined) return server._path;
      return {
        address: "127.0.0.1",
        family: "IPv4",
        port: server._nativeTransport
          ? __quench_tcp_bound_port(server._nativeId)
          : server._port,
      };
    };
    server.listen = (_port, host, callback) => {
      if (typeof host === "function") {
        callback = host;
        host = undefined;
      }
      if (typeof _port === "function") {
        callback = _port;
        _port = 0;
      }
      const listenOptions = _port && typeof _port === "object"
        ? _port
        : { port: _port, host };
      const adoptedBound = _port?.constructor?.name === "BoundSocket"
        ? _port
        : undefined;
      if (adoptedBound) {
        adoptedBound._assertOpen();
        adoptedBound._adopted = true;
        server._port = adoptedBound._port;
        server._path = adoptedBound._path;
      }
      const requestedPort = Number(listenOptions.port || 0);
      const listenHost = listenOptions.host || "0.0.0.0";
      if (
        typeof listenHost === "string" &&
        isIPv4(listenHost) &&
        !["0.0.0.0", "127.0.0.1"].includes(listenHost)
      ) {
        const error = new Error("Cannot assign requested address");
        error.code = "EADDRNOTAVAIL";
        error.address = listenHost;
        error.port = requestedPort;
        error.syscall = "listen";
        queueMicrotask(() => server.emit("error", error));
        return server;
      }
      const occupied = [...__quenchNetServers].some(
        (candidate) =>
          candidate.listening &&
          !candidate._nativeTransport &&
          requestedPort !== 0 &&
          candidate.address().port === requestedPort,
      );
      if (occupied) {
        const error = new Error(
          `listen EADDRINUSE: address already in use 127.0.0.1:${requestedPort}`,
        );
        error.code = "EADDRINUSE";
        error.syscall = "listen";
        queueMicrotask(() => server.emit("error", error));
        return server;
      }
      if (__quenchNativeTransportRequested(listenOptions)) {
        server._nativeId = __quench_tcp_bind(
          listenOptions.host || "127.0.0.1",
          Number(listenOptions.port || 0),
        );
        server._nativeTransport = true;
      } else if (!adoptedBound) {
        server._port = Number(listenOptions.port) ||
          __quenchNextEphemeralPort++;
      }
      server.listening = true;
      __quenchNetServers.add(server);
      queueMicrotask(() => {
        globalThis.__quench_work_generation =
          (globalThis.__quench_work_generation || 0) + 1;
        server.emit("listening");
        if (typeof callback === "function") callback.call(server);
      });
      return server;
    };
    server.close = (callback) => {
      if (!server.listening) return server;
      server.listening = false;
      server._closeRequested = true;
      __quenchNetServers.delete(server);
      if (server._nativeId) {
        __quench_tcp_close(server._nativeId);
        server._nativeId = 0;
      }
      let callbackCalled = false;
      const finish = () => {
        if (!server._closeRequested || server._connections.size) return;
        server._closeRequested = false;
        server.emit("close");
        if (typeof callback === "function" && !callbackCalled) {
          callbackCalled = true;
          callback.call(server);
        }
      };
      server._finishClose = finish;
      finish();
      return server;
    };
    server.unref = () => server;
    if (typeof handler === "function") server.on("connection", handler);
    return server;
  },
  Server: function Server(options, handler) {
    return __quenchNetModule.createServer(options, handler);
  },
};
const __quenchNativeTransportRequested = (options) =>
  options?.__quenchNativeTransport === true ||
  globalThis.process?.env?.QUENCH_NATIVE_TRANSPORT === "1";
globalThis.__quench_io_poll = () => {
  for (const server of __quenchNetServers) {
    if (!server._nativeTransport || !server._nativeId) continue;
    for (;;) {
      const nativeId = __quench_tcp_accept(server._nativeId);
      if (!nativeId) break;
      const socket = new __quenchNetModule.Socket();
      socket._nativeId = nativeId;
      socket._nativeConnected = true;
      socket.localAddress = "127.0.0.1";
      socket.localPort = __quench_tcp_bound_port(server._nativeId);
      socket.remoteAddress = "127.0.0.1";
      socket.remotePort = __quench_tcp_peer_port(nativeId);
      __quenchNativeSockets.add(socket);
      server._connections.add(socket);
      socket.once("close", () => {
        server._connections.delete(socket);
        server._finishClose?.();
      });
      server.emit("connection", socket);
    }
  }
  for (const socket of __quenchNativeSockets) {
    if (socket.destroyed || !socket._nativeId) continue;
    const state = __quench_tcp_readable(socket._nativeId);
    if (state === 1) {
      const bytes = __quench_tcp_read(socket._nativeId);
      if (bytes.length) {
        socket.bytesRead += bytes.length;
        socket.emit("data", NodeBuffer.from(bytes));
      }
    } else if (state === 2 && !socket._nativeEnded) {
      socket._nativeEnded = true;
      socket.emit("end");
    }
  }
};
globalThis.__quench_require_part_01 = (name, specifier) =>
  name === "net" ? __quenchNetModule : undefined;
