const __quenchOriginalRequireWithDgram = globalThis.require;
const __quenchDgramStateSymbol = Symbol.for("quench.dgram.state");
const __quenchDgramBoundPorts = new Set();
let __quenchDgramNextPort = 40000;
const __quenchDgramBind = (socket, type, port, address, callback) => {
  if (socket._bound)
    throw Object.assign(new Error("Socket is already bound"), {
      code: "ERR_SOCKET_ALREADY_BOUND"
    });
  if (typeof address === "function") callback = address;
  const resolvedPort =
    typeof port === "number" && port > 0 ? port : __quenchDgramNextPort++;
  if (__quenchDgramBoundPorts.has(resolvedPort))
    throw Object.assign(new Error("bind EADDRINUSE"), {
      code: "EADDRINUSE",
      syscall: "bind"
    });
  socket._bound = true;
  socket._bindPending = true;
  __quenchDgramBoundPorts.add(resolvedPort);
  socket._address = {
    address:
      typeof address === "string"
        ? address
        : type === "udp6"
          ? "::"
          : "0.0.0.0",
    family: type === "udp6" ? "IPv6" : "IPv4",
    port: resolvedPort
  };
  queueMicrotask(() => {
    socket._bindPending = false;
    callback?.call(socket);
  });
  queueMicrotask(() => socket.emit("listening"));
  return socket;
};
const __quenchDgramSend = (socket, message, ...args) => {
  if (
    typeof message !== "string" &&
    !(message instanceof NodeBuffer) &&
    !Array.isArray(message) &&
    !ArrayBuffer.isView(message)
  )
    throw Object.assign(
      new TypeError('The "buffer" argument must be a Buffer'),
      {
        code: "ERR_INVALID_ARG_TYPE"
      }
    );
  const addressIndex = args.length >= 4 ? 3 : 1;
  const address = args[addressIndex];
  if (
    address !== undefined &&
    address !== null &&
    address !== "" &&
    typeof address !== "function" &&
    typeof address !== "string"
  )
    throw Object.assign(
      new TypeError('The "address" argument must be of type string'),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  const callback = args.at(-1);
  const hasOffset = args.length >= 5 || (socket._connected && args.length >= 3);
  const payload = Array.isArray(message) ? NodeBuffer.concat(message) : message;
  const length = hasOffset ? args[1] : payload.byteLength;
  queueMicrotask(() => {
    if (!socket._bound) return;
    socket.emit("message", payload, socket.address());
    if (typeof callback === "function") callback(null, length);
  });
  return socket;
};
const __quenchDgramSendTo = (socket, message, ...args) => {
  if (args[0] === undefined)
    throw Object.assign(
      new TypeError(
        'The "offset" argument must be of type number. Received undefined'
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  if (typeof args[0] !== "number")
    throw Object.assign(
      new TypeError(
        `The "offset" argument must be of type number. Received type string ('${args[0]}')`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  if (typeof args[1] !== "number")
    throw Object.assign(
      new TypeError(
        `The "length" argument must be of type number. Received type string ('${args[1]}')`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  if (typeof args[3] !== "string")
    throw Object.assign(
      new TypeError(
        `The "address" argument must be of type string. Received type boolean (${args[3]})`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  if (typeof args[2] !== "number")
    throw Object.assign(
      new TypeError(
        `The "port" argument must be of type number. Received type boolean (${args[2]})`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  return __quenchDgramSend(socket, message, ...args);
};
const __quenchDgramConnect = (socket, port, address, callback) => {
  if (typeof address === "function") {
    callback = address;
    address = "127.0.0.1";
  }
  if (!Number.isInteger(port) || port <= 0 || port >= 65536)
    throw Object.assign(new RangeError("Port should be > 0 and < 65536"), {
      code: "ERR_SOCKET_BAD_PORT"
    });
  if (socket._connected || socket._connecting)
    throw Object.assign(new Error("Already connected"), {
      code: "ERR_SOCKET_DGRAM_IS_CONNECTED"
    });
  socket._connecting = true;
  socket._remote = { address, port };
  setTimeout(() => {
    socket._connecting = false;
    socket._connected = true;
    callback?.();
    socket.emit("connect");
  });
  return socket;
};
const __quenchDgramDisconnect = (socket) => {
  if (!socket._connected)
    throw Object.assign(new Error("Not connected"), {
      code: "ERR_SOCKET_DGRAM_NOT_CONNECTED"
    });
  socket._connected = false;
  socket._remote = undefined;
  return socket;
};
const __quenchDgramRemoteAddress = (socket) => {
  if (!socket._connected)
    throw Object.assign(new Error("Not connected"), {
      code: "ERR_SOCKET_DGRAM_NOT_CONNECTED"
    });
  return { ...socket._remote, family: "IPv4" };
};
const __quenchDgramClose = (socket, callback) => {
  socket._bound = false;
  if (socket._address) __quenchDgramBoundPorts.delete(socket._address.port);
  callback?.();
  queueMicrotask(() => socket.emit("close"));
  return socket;
};
const __quenchDgramAddress = (socket, type) => {
  if (!socket._bound) throw new Error("getsockname EBADF");
  return (
    socket._address || {
      address: "0.0.0.0",
      family: type === "udp6" ? "IPv6" : "IPv4",
      port: 0
    }
  );
};
const __quenchDgramOn = (socket, listeners, event, callback) => {
  (listeners[event] ||= []).push(callback);
  return socket;
};
const __quenchDgramOnce = (socket, listeners, event, callback) => {
  const wrapper = (...args) => {
    listeners[event] = (listeners[event] || []).filter(
      (listener) => listener !== wrapper
    );
    callback.apply(socket, args);
  };
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
    bind: (port, address, callback) =>
      __quenchDgramBind(socket, type, port, address, callback),
    bindSync: (options = {}) => {
      if (socket._bound)
        throw Object.assign(new Error("Socket is already bound"), {
          code: "ERR_SOCKET_ALREADY_BOUND"
        });
      if (
        options !== undefined &&
        (options === null || typeof options !== "object")
      )
        throw Object.assign(
          new TypeError('The "options" argument must be of type object'),
          {
            code: "ERR_INVALID_ARG_TYPE"
          }
        );
      const config = options;
      const port = config?.port ?? 0;
      const resolvedPort = port || __quenchDgramNextPort++;
      const address = config?.address || (type === "udp6" ? "::" : "0.0.0.0");
      if (!Number.isInteger(port) || port < 0 || port > 65535)
        throw Object.assign(new RangeError("Port should be >= 0 and < 65536"), {
          code: "ERR_SOCKET_BAD_PORT"
        });
      if (typeof address !== "string")
        throw Object.assign(
          new TypeError('The "address" argument must be of type string'),
          {
            code: "ERR_INVALID_ARG_TYPE"
          }
        );
      if (address === "localhost")
        throw Object.assign(new TypeError("Invalid IP address"), {
          code: "ERR_INVALID_ARG_VALUE"
        });
      if (
        options?.sendBlockList?.check?.(
          address,
          type === "udp6" ? "ipv6" : "ipv4"
        )
      )
        throw Object.assign(new Error("IP is blocked"), {
          code: "ERR_IP_BLOCKED"
        });
      if (__quenchDgramBoundPorts.has(resolvedPort))
        throw Object.assign(new Error("bind EADDRINUSE"), {
          code: "EADDRINUSE",
          syscall: "bind"
        });
      socket._bound = true;
      __quenchDgramBoundPorts.add(resolvedPort);
      socket._address = {
        address,
        family: type === "udp6" ? "IPv6" : "IPv4",
        port: resolvedPort
      };
      queueMicrotask(() => socket._bound && socket.emit("listening"));
      return socket._address;
    },
    send: (message, ...args) => __quenchDgramSend(socket, message, ...args),
    sendto: (message, ...args) => __quenchDgramSendTo(socket, message, ...args),
    connect: (port, address, callback) =>
      __quenchDgramConnect(socket, port, address, callback),
    connectSync: (port, address = "127.0.0.1") => {
      if (socket._bindPending)
        throw Object.assign(new Error("Socket is already bound"), {
          code: "ERR_SOCKET_ALREADY_BOUND"
        });
      if (!Number.isInteger(port) || port <= 0 || port >= 65536)
        throw Object.assign(new RangeError("Port should be > 0 and < 65536"), {
          code: "ERR_SOCKET_BAD_PORT"
        });
      if (typeof address !== "string")
        throw Object.assign(
          new TypeError('The "address" argument must be of type string'),
          {
            code: "ERR_INVALID_ARG_TYPE"
          }
        );
      if (address === "localhost")
        throw Object.assign(new TypeError("Invalid IP address"), {
          code: "ERR_INVALID_ARG_VALUE"
        });
      if (socket._connected)
        throw Object.assign(new Error("Already connected"), {
          code: "ERR_SOCKET_DGRAM_IS_CONNECTED"
        });
      if (
        options?.sendBlockList?.check?.(
          address,
          type === "udp6" ? "ipv6" : "ipv4"
        )
      )
        throw Object.assign(new Error("IP is blocked"), {
          code: "ERR_IP_BLOCKED"
        });
      if (!socket._bound) {
        socket._bound = true;
        const localPort = __quenchDgramNextPort++;
        __quenchDgramBoundPorts.add(localPort);
        socket._address = {
          address: type === "udp6" ? "::" : "0.0.0.0",
          family: type === "udp6" ? "IPv6" : "IPv4",
          port: localPort
        };
      }
      socket._connected = true;
      socket._remote = { address, port };
      queueMicrotask(() => socket._bound && socket.emit("connect"));
    },
    disconnect: () => __quenchDgramDisconnect(socket),
    remoteAddress: () => __quenchDgramRemoteAddress(socket),
    getRecvBufferSize: () => options.recvBufferSize || 0,
    getSendBufferSize: () => options.sendBufferSize || 0,
    close: (callback) => __quenchDgramClose(socket, callback),
    address: () => __quenchDgramAddress(socket, type),
    on: (event, callback) =>
      __quenchDgramOn(socket, listeners, event, callback),
    once: (event, callback) =>
      __quenchDgramOnce(socket, listeners, event, callback),
    emit: (event, ...args) => __quenchDgramEmit(socket, listeners, event, args),
    ref: () => socket,
    unref: () => socket
  };
  socket[__quenchDgramStateSymbol] = { handle: { fd: 0 } };
  return socket;
};
const __quenchDgramValidateType = (type) => {
  if (type === "udp4" || type === "udp6") return type;
  throw Object.assign(
    new TypeError("Bad socket type specified. Valid types are: udp4, udp6"),
    { code: "ERR_SOCKET_BAD_TYPE" }
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
    )
      return __quenchDgramValidateType(type);
    const requested =
      typeof type === "string" ? type : type?.type || options?.type;
    const config = typeof type === "object" ? type : options;
    for (const name of ["recvBufferSize", "sendBufferSize"]) {
      if (config?.[name] !== undefined && typeof config[name] !== "number")
        throw Object.assign(
          new TypeError(`The "${name}" option must be a number`),
          {
            code: "ERR_INVALID_ARG_TYPE"
          }
        );
    }
    return __quenchDgramSocket(__quenchDgramValidateType(requested), config);
  }
};
globalThis.require = (specifier) =>
  String(specifier).replace(/^node:/, "") === "dgram"
    ? __quenchDgram
    : specifier === "internal/dgram"
      ? { kStateSymbol: __quenchDgramStateSymbol }
      : __quenchOriginalRequireWithDgram(specifier);
