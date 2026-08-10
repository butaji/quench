//! Polyfill: `network`

pub const JS: &str = r#"let __quenchNetAutoSelectFamily = false;
const __quenchNetNormalizedArgsSymbol =
  (globalThis.__quenchNetNormalizedArgsSymbol ||= Symbol("normalizedArgs"));
const __quenchNetFamilyTimeoutFlag = globalThis.__quench_argv?.find?.((value) =>
  value.startsWith("--network-family-autoselection-attempt-timeout=")
);
let __quenchNetAutoSelectFamilyAttemptTimeout = __quenchNetFamilyTimeoutFlag
  ? Math.max(
    10,
    Number(__quenchNetFamilyTimeoutFlag.split("=").pop()) * 5,
  )
  : 2500;
const __quenchNetModule = {
  _normalizeArgs(input) {
    if (input?.[__quenchNetNormalizedArgsSymbol]) return input;
    const args = [input?.[0] || {}, input?.[1] ?? null];
    args[__quenchNetNormalizedArgsSymbol] = true;
    return args;
  },
  isIP,
  isIPv4,
  isIPv6,
  getDefaultAutoSelectFamily: () => __quenchNetAutoSelectFamily,
  setDefaultAutoSelectFamily: (value) => {
    if (typeof value !== "boolean") {
      throw Object.assign(new TypeError('The "value" argument must be of type boolean'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    __quenchNetAutoSelectFamily = value;
  },
  getDefaultAutoSelectFamilyAttemptTimeout: () =>
    __quenchNetAutoSelectFamilyAttemptTimeout,
  Socket: __quenchNetSocket,
  createConnection: (options, callback) => {
    globalThis.__quenchValidateConnectionOptions(options);
    if (options.autoSelectFamilyAttemptTimeout !== undefined) {
      const value = options.autoSelectFamilyAttemptTimeout;
      if (!Number.isInteger(value) || value <= 0) {
        throw Object.assign(new RangeError('The "value" argument is out of range'), { code: "ERR_OUT_OF_RANGE" });
      }
    }
    const socket = new __quenchNetModule.Socket();
    socket._handle = { setKeepAlive: () => {} };
    const connect = (resolvedOptions) =>
      socket.connect(resolvedOptions, callback);
    if (
      typeof options.lookup === "function" &&
      typeof options.host === "string" &&
      !isIPv4(options.host) &&
      !isIPv6(options.host)
    ) {
      const autoSelect = options.autoSelectFamily ??
        __quenchNetAutoSelectFamily;
      options.lookup(
        options.host,
        { all: Boolean(autoSelect), family: options.family },
        (error, value, family) => {
          if (error) {
            queueMicrotask(() => socket.emit("error", error));
            return;
          }
          const addresses = Array.isArray(value) ? value : [{
            address: value,
            family,
          }];
          if (autoSelect) {
            socket.autoSelectFamilyAttemptedAddresses = addresses.map((entry) =>
              `${entry.address}:${options.port}`
            );
          }
          if (
            options.blockList?.check?.(addresses[0]?.address) &&
            addresses.every((entry) => options.blockList.check(entry.address))
          ) {
            const error = new Error(
              `Cannot connect to ${addresses[0].address}`,
            );
            error.code = "ERR_IP_BLOCKED";
            queueMicrotask(() => socket.emit("error", error));
            return;
          }
          const selected = autoSelect
            ? addresses.find((entry) =>
              [...__quenchNetServers].some((server) =>
                server.listening &&
                server.address().address === entry.address &&
                server.address().port === Number(options.port)
              )
            )
            : addresses[0];
          if (!selected) {
            const noAddress = new AggregateError(
              addresses.map((entry) => {
                const error = new Error(
                  `connect ECONNREFUSED ${entry.address}:${options.port}`,
                );
                error.code = "ECONNREFUSED";
                return error;
              }),
              "All connection attempts failed",
            );
            socket.emit("error", noAddress);
            return;
          }
          if (autoSelect) {
            socket.autoSelectFamilyAttemptedAddresses = addresses.map((entry) =>
              `${entry.address}:${options.port}`
            );
          }
          connect({
            ...options,
            host: selected.address,
            _resolvedAddress: true,
          });
        },
      );
    } else connect(options);
    if (__quenchNativeTransportRequested(options)) return socket;
    return socket;
  },
  setDefaultAutoSelectFamilyAttemptTimeout: (value) => {
    if (!Number.isInteger(value) || value <= 0) {
      throw Object.assign(new RangeError('The "value" argument is out of range'), { code: "ERR_OUT_OF_RANGE" });
    }
    __quenchNetAutoSelectFamilyAttemptTimeout = Math.max(10, value);
  },
  BlockList: __quenchNetBlockList,
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
        throw Object.assign(new TypeError("options must be an object"), { code: "ERR_INVALID_ARG_TYPE" });
      }
      const host = options.host ?? (options.ipv6Only ? "::" : "0.0.0.0");
      if (options.path !== undefined) {
        if (typeof options.path !== "string") {
          throw Object.assign(new TypeError("path must be a string"), { code: "ERR_INVALID_ARG_TYPE" });
        }
        if (options.path.startsWith("\0") && process.platform !== "linux") {
          throw Object.assign(new TypeError("abstract socket paths are Linux-only"), { code: "ERR_INVALID_ARG_VALUE" });
        }
        if (
          options.host !== undefined ||
          options.port !== undefined ||
          options.ipv6Only !== undefined ||
          options.reusePort !== undefined
        ) {
          throw Object.assign(new TypeError("path cannot be combined with TCP options"), { code: "ERR_INVALID_ARG_VALUE" });
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
        throw Object.assign(new TypeError("host must be a string"), { code: "ERR_INVALID_ARG_TYPE" });
      }
      if (!isIPv4(host) && !isIPv6(host) && host !== "0.0.0.0") {
        throw Object.assign(new TypeError("host must be an IPv4 address"), { code: "ERR_INVALID_ARG_VALUE" });
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
    server._host = "127.0.0.1";
    server._ipv6Only = options?.ipv6Only === true;
    server._path = undefined;
    server._handle = { close: () => {} };
    server.keepAlive = options?.keepAlive;
    server.keepAliveInitialDelay = options?.keepAliveInitialDelay;
    server._allowHalfOpen = options?.allowHalfOpen === true;
    server.address = () => {
      if (!server.listening) return null;
      if (server._path !== undefined) return server._path;
      return {
        address: server._host,
        family: isIPv6(server._host) ? "IPv6" : "IPv4",
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
      const requestedPortValue = listenOptions.port;
      if (
        requestedPortValue !== undefined &&
        (typeof requestedPortValue !== "number" ||
          !Number.isInteger(requestedPortValue) ||
          requestedPortValue < 0 || requestedPortValue > 65535)
      ) {
        throw Object.assign(new RangeError("Port should be >= 0 and < 65536"), { code: "ERR_SOCKET_BAD_PORT" });
      }
      if (typeof _port === "string" && host === undefined) {
        const error = new Error(`listen ${_port}: no such file or directory`);
        error.code = "ENOENT";
        error.syscall = "listen";
        error.address = _port;
        queueMicrotask(() => server.emit("error", error));
        return server;
      }
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
      const boundHost = listenHost === "0.0.0.0" ? "127.0.0.1" : listenHost;
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
          candidate.address().port === requestedPort &&
          (candidate.address().address === boundHost ||
            isIPv4(candidate.address().address) === isIPv4(boundHost)),
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
      server._host = boundHost;
      server._ipv6Only = listenOptions.ipv6Only === true;
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
globalThis.__quench_require_part_01 = (name, specifier) => {
  if (name === "net") return __quenchNetModule;
  if (name === "net/promises") return globalThis.__quenchNetPromisesModule;
  if (name === "internal/net") {
    return { normalizedArgsSymbol: __quenchNetNormalizedArgsSymbol };
  }
};
"#;
