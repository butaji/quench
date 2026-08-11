//! Polyfill: `dgram-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchDgramOnce = (socket, listeners, event, callback) => {
  const wrapper = (...args) => {
    listeners[event] = (listeners[event] || []).filter(
      (listener) => listener !== wrapper,
    );
    callback.apply(socket, args);
  };
  wrapper.__quenchOriginalListener = callback;
  return __quenchDgramOn(socket, listeners, event, wrapper);
};
const __quenchDgramEmit = (socket, listeners, event, args) => {
  for (const callback of listeners[event] || []) callback.apply(socket, args);
  return socket;
};
const __quenchDgramSocket = (type = "udp4", options = {}) => {
  const listeners = {};
  const socket = {
    type,
    _sendBlockList: options?.sendBlockList,
    _receiveBlockList: options?.receiveBlockList,
    _reusePort: options?.reusePort === true,
    _lookup: options?.lookup,
    bind: (port, address, callback) => {
      if (socket._bound || socket._bindPending) {
        throw Object.assign(new Error("Socket is already bound"), {
          code: "ERR_SOCKET_ALREADY_BOUND",
        });
      }
      if (typeof address === "function") {
        callback = address;
        address = undefined;
      }
      if (port && typeof port === "object" && port.fd !== undefined) {
        if (globalThis.__quenchDgramActiveFds.has(port.fd)) {
          throw Object.assign(new Error("open EEXIST"), {
            code: "EEXIST",
          });
        }
        if (!globalThis.__quenchDgramUdpFds.has(port.fd)) {
          throw Object.assign(new TypeError("Unsupported fd type: TCP"), {
            code: "ERR_INVALID_FD_TYPE",
          });
        }
        socket._bound = true;
        __quenchDgramSockets.add(socket);
        socket[__quenchDgramStateSymbol].handle.fd = port.fd;
        globalThis.__quenchDgramActiveFds.add(port.fd);
        socket._address = {
          ...(globalThis.__quenchDgramUdpHandleInfo.get(port.fd) || {}),
          address: port.address ||
            globalThis.__quenchDgramUdpHandleInfo.get(port.fd)?.address ||
            (type === "udp6" ? "::" : "0.0.0.0"),
          family: globalThis.__quenchDgramUdpHandleInfo.get(port.fd)?.family ||
            (type === "udp6" ? "IPv6" : "IPv4"),
          port: port.port ||
            globalThis.__quenchDgramUdpHandleInfo.get(port.fd)?.port ||
            __quenchDgramNextPort++,
        };
        __quenchDgramBoundPorts.add(socket._address.port);
        __quenchDgramClosedPorts.delete(socket._address.port);
        setImmediate(() => {
          callback?.call(socket);
          socket.emit("listening");
        });
        return socket;
      }
      if (typeof address === "function") {
        callback = address;
        address = type === "udp6" ? "::" : "0.0.0.0";
      }
      const lookupAddress = address || (type === "udp6" ? "::" : "0.0.0.0");
      socket._bindPending = true;
      const isIpv4Literal = typeof lookupAddress === "string" &&
        /^\d{1,3}(?:\.\d{1,3}){3}$/.test(lookupAddress);
      const isIpv6Literal = typeof lookupAddress === "string" &&
        lookupAddress.includes(":");
      const isHostname = typeof lookupAddress === "string" &&
        lookupAddress !== "localhost" &&
        (type === "udp4" ? !isIpv4Literal : !isIpv6Literal);
      const lookup = socket._lookup ||
        (isHostname
          ? globalThis.require("dns").lookup
          : socket[__quenchDgramStateSymbol].handle.lookup);
      const lookupCallback = (error, resolvedAddress) => {
        if (error) {
          socket._bindPending = false;
          if (!socket._closed) socket.emit("error", error);
          return;
        }
        socket._bindPending = false;
        if (!socket._bound && !socket._closed) {
          __quenchDgramBind(socket, type, port, resolvedAddress, callback);
        }
      };
      if (socket._lookup || isHostname) {
        lookup(lookupAddress, type === "udp6" ? 6 : 4, lookupCallback);
      } else {
        lookup(lookupAddress, lookupCallback);
      }
      return socket;
    },
    bindSync: (options = {}) => {
      if (socket._bound) {
        throw Object.assign(new Error("Socket is already bound"), {
          code: "ERR_SOCKET_ALREADY_BOUND",
        });
      }
      if (
        options !== undefined &&
        (options === null || typeof options !== "object")
      ) {
        throw Object.assign(
          new TypeError('The "options" argument must be of type object'),
          {
            code: "ERR_INVALID_ARG_TYPE",
          },
        );
      }
      const config = options;
      const port = config?.port ?? 0;
      const resolvedPort = port || __quenchDgramNextPort++;
      const address = config?.address || (type === "udp6" ? "::" : "0.0.0.0");
      if (!Number.isInteger(port) || port < 0 || port > 65535) {
        throw Object.assign(new RangeError("Port should be >= 0 and < 65536"), {
          code: "ERR_SOCKET_BAD_PORT",
        });
      }
      if (typeof address !== "string") {
        throw Object.assign(
          new TypeError('The "address" argument must be of type string'),
          {
            code: "ERR_INVALID_ARG_TYPE",
          },
        );
      }
      if (address === "localhost") {
        throw Object.assign(new TypeError("Invalid IP address"), {
          code: "ERR_INVALID_ARG_VALUE",
        });
      }
      if (
        options?.sendBlockList?.check?.(
          address,
          type === "udp6" ? "ipv6" : "ipv4",
        )
      ) {
        throw Object.assign(new Error("IP is blocked"), {
          code: "ERR_IP_BLOCKED",
        });
      }
      if (__quenchDgramBoundPorts.has(resolvedPort)) {
        throw Object.assign(new Error("bind EADDRINUSE"), {
          code: "EADDRINUSE",
          syscall: "bind",
        });
      }
      socket._bound = true;
      __quenchDgramSockets.add(socket);
      __quenchDgramBoundPorts.add(resolvedPort);
      __quenchDgramClosedPorts.delete(resolvedPort);
      socket._address = {
        address,
        family: type === "udp6" ? "IPv6" : "IPv4",
        port: resolvedPort,
      };
      setImmediate(() => socket._bound && socket.emit("listening"));
      return socket._address;
    },
    send: (message, ...args) => __quenchDgramSend(socket, message, ...args),
    sendto: (message, ...args) => __quenchDgramSendTo(socket, message, ...args),
    connect: (port, address, callback) =>
      __quenchDgramConnect(socket, port, address, callback),
    connectSync: (port, address = type === "udp6" ? "::1" : "127.0.0.1") => {
      if (socket._bindPending) {
        throw Object.assign(new Error("Socket is already bound"), {
          code: "ERR_SOCKET_ALREADY_BOUND",
        });
      }
      if (!Number.isInteger(port) || port <= 0 || port >= 65536) {
        throw Object.assign(new RangeError("Port should be > 0 and < 65536"), {
          code: "ERR_SOCKET_BAD_PORT",
        });
      }
      if (typeof address !== "string") {
        throw Object.assign(
          new TypeError('The "address" argument must be of type string'),
          {
            code: "ERR_INVALID_ARG_TYPE",
          },
        );
      }
      if (address === "localhost") {
        throw Object.assign(new TypeError("Invalid IP address"), {
          code: "ERR_INVALID_ARG_VALUE",
        });
      }
      if (socket._connected) {
        throw Object.assign(new Error("Already connected"), {
          code: "ERR_SOCKET_DGRAM_IS_CONNECTED",
        });
      }
      if (
        options?.sendBlockList?.check?.(
          address,
          type === "udp6" ? "ipv6" : "ipv4",
        )
      ) {
        throw Object.assign(new Error("IP is blocked"), {
          code: "ERR_IP_BLOCKED",
        });
      }
      if (!socket._bound) {
        socket._bound = true;
        __quenchDgramSockets.add(socket);
        const localPort = __quenchDgramNextPort++;
        __quenchDgramBoundPorts.add(localPort);
        __quenchDgramClosedPorts.delete(localPort);
        socket._address = {
          address: type === "udp6" ? "::" : "0.0.0.0",
          family: type === "udp6" ? "IPv6" : "IPv4",
          port: localPort,
        };
      }
      socket._connected = true;
      socket._remote = { address, port };
      setImmediate(() => socket._bound && socket.emit("connect"));
    },
    disconnect: () => __quenchDgramDisconnect(socket),
    remoteAddress: () => __quenchDgramRemoteAddress(socket),
    getRecvBufferSize: () => {
      if (!socket._bound && options.recvBufferSize === undefined) {
        throw __quenchDgramBufferError("recv", "EBADF", "bad file descriptor");
      }
      return socket._recvBufferSize || options.recvBufferSize || 0;
    },
    getSendBufferSize: () => {
      if (!socket._bound && options.sendBufferSize === undefined) {
        throw __quenchDgramBufferError("send", "EBADF", "bad file descriptor");
      }
      return socket._sendBufferSize || options.sendBufferSize || 0;
    },
    getSendQueueSize: () => socket._sendQueueSize || 0,
    getSendQueueCount: () => socket._sendQueueCount || 0,
    setRecvBufferSize: (value) => {
      if (!socket._bound) {
        throw __quenchDgramBufferError("recv", "EBADF", "bad file descriptor");
      }
      if (!Number.isInteger(value) || value <= 0) {
        throw Object.assign(
          new TypeError("Buffer size must be a positive integer"),
          {
            code: "ERR_SOCKET_BAD_BUFFER_SIZE",
          },
        );
      }
      if (value > 0x7fffffff) {
        throw __quenchDgramBufferError("recv", "EINVAL", "invalid argument");
      }
      socket._recvBufferSize = value * 2;
    },
    setSendBufferSize: (value) => {
      if (!socket._bound) {
        throw __quenchDgramBufferError("send", "EBADF", "bad file descriptor");
      }
      if (!Number.isInteger(value) || value <= 0) {
        throw Object.assign(
          new TypeError("Buffer size must be a positive integer"),
          {
            code: "ERR_SOCKET_BAD_BUFFER_SIZE",
          },
        );
      }
      if (value > 0x7fffffff) {
        throw __quenchDgramBufferError("send", "EINVAL", "invalid argument");
      }
      socket._sendBufferSize = value * 2;
    },
    setBroadcast: (value) => {
      if (!socket._bound) throw new Error("setBroadcast EBADF");
      return value;
    },
    setTTL: (value) => {
      if (!socket._bound) throw new Error("setTTL EBADF");
      if (typeof value !== "number") {
        throw Object.assign(
          new TypeError(
            `The "ttl" argument must be of type number. Received type string ('${value}')`,
          ),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      if (value <= 0 || value >= 256) throw new Error("setTTL EINVAL");
      return value;
    },
    setMulticastLoopback: (value) => {
      if (!socket._bound) throw new Error("setMulticastLoopback EBADF");
      return value;
    },
    setMulticastInterface: (address) => {
      if (!socket._bound) throw new Error("Not running");
      if (typeof address !== "string") {
        throw new TypeError("interfaceAddress must be a string");
      }
      const firstOctet = Number(address.split(".", 1)[0]);
      if (
        (Number.isInteger(firstOctet) &&
          firstOctet >= 224 &&
          firstOctet <= 239) ||
        address === "::" ||
        address === "" ||
        address === "undefined"
      ) {
        throw new Error("EINVAL");
      }
      return socket;
    },
    setMulticastTTL: (value) => {
      if (!socket._bound) throw new Error("setMulticastTTL EBADF");
      if (typeof value !== "number") {
        throw Object.assign(
          new TypeError(
            `The "ttl" argument must be of type number. Received type string ('${value}')`,
          ),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      if (value <= 0 || value >= 256) throw new Error("setMulticastTTL EINVAL");
      return value;
    },
    close: (callback) => __quenchDgramClose(socket, callback),
    address: () => __quenchDgramAddress(socket, type),
    on: (event, callback) =>
      __quenchDgramOn(socket, listeners, event, callback),
    removeListener: (event, callback) => {
      listeners[event] = (listeners[event] || []).filter(
        (listener) =>
          listener !== callback &&
          listener.__quenchOriginalListener !== callback,
      );
      return socket;
    },
    once: (event, callback) =>
      __quenchDgramOnce(socket, listeners, event, callback),
    emit: (event, ...args) => __quenchDgramEmit(socket, listeners, event, args),
    ref: () => socket,
    unref: () => socket,
  };
  Object.assign(socket, __quenchDgramMembershipMethods(socket));
  socket[__quenchDgramStateSymbol] = {
    handle: {
      fd: 0,
      lookup(address, familyOrCallback, maybeCallback) {
        const callback = typeof familyOrCallback === "function"
          ? familyOrCallback
          : maybeCallback;
        setImmediate(() => {
          callback(null, address === "localhost" ? "127.0.0.1" : address);
        });
      },
      onmessage(status) {
        if (status >= 0) return;
        const error = Object.assign(new Error("recvmsg"), {
          syscall: "recvmsg",
          errno: status,
        });
        socket.emit("error", error);
      },
    },
  };
  if (
    options?.signal !== undefined &&
    (!options.signal || typeof options.signal.addEventListener !== "function")
  ) {
    throw Object.assign(
      new TypeError('The "signal" option must be an AbortSignal'),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (options?.signal) {
    const closeOnAbort = () => socket.close();
    if (options.signal.aborted) queueMicrotask(closeOnAbort);
    else options.signal.addEventListener("abort", closeOnAbort, { once: true });
  }
  return socket;
};
const __quenchDgramValidateType = (type) => {
  if (type === "udp4" || type === "udp6") return type;
  throw Object.assign(
    new TypeError("Bad socket type specified. Valid types are: udp4, udp6"),
    { code: "ERR_SOCKET_BAD_TYPE" },
  );
};
const __quenchDgram = {
  createSocket: function createSocket(type, options) {
    if (
      type === null ||
      (type === undefined && options === undefined) ||
      (typeof type !== "string" && typeof type !== "object") ||
      Array.isArray(type) ||
      type instanceof String
    ) {
      return __quenchDgramValidateType(type);
    }
    const requested = typeof type === "string"
      ? type
      : type?.type || options?.type;
    const config = typeof type === "object" ? type : options;
    if (config?.lookup !== undefined && typeof config.lookup !== "function") {
      throw Object.assign(
        new TypeError(
          `The "lookup" argument must be of type function.${
            __quenchDgramTypeDetail(
              config.lookup,
            )
          }`,
        ),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    for (const name of ["recvBufferSize", "sendBufferSize"]) {
      if (config?.[name] !== undefined && typeof config[name] !== "number") {
        throw Object.assign(
          new TypeError(`The "${name}" option must be a number`),
          {
            code: "ERR_INVALID_ARG_TYPE",
          },
        );
      }
    }
    return __quenchDgramSocket(__quenchDgramValidateType(requested), config);
  },
};
globalThis.require = (specifier) =>
  String(specifier).replace(/^node:/, "") === "dgram"
    ? __quenchDgram
    : specifier === "internal/dgram"
    ? {
      kStateSymbol: __quenchDgramStateSymbol,
      _createSocketHandle(address, port, type) {
        const fd = arguments[3];
        if (fd !== undefined) {
          if (!globalThis.__quenchDgramUdpFds.has(fd)) return -9;
          const adopted = new globalThis.__quenchDgramUDPClass();
          adopted.fd = fd;
          return adopted;
        }
        const handle = new globalThis.__quenchDgramUDPClass();
        if (address === null) return handle;
        const result = handle.bind(address, port, 0);
        return result < 0 ? result : handle;
      },
    }
    : __quenchOriginalRequireWithDgram(specifier);
"#);
