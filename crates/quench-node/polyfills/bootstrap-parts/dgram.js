/* eslint-disable max-lines-per-function, complexity */
const __quenchOriginalRequireWithDgram = globalThis.require;
const __quenchDgramStateSymbol = Symbol.for("quench.dgram.state");
const __quenchDgramBoundPorts = new Set();
const __quenchDgramClosedPorts = new Set();
const __quenchDgramSockets = new Set();
let __quenchDgramNextPort = 40000;
const __quenchDgramTypeDetail = (value) => {
  if (value === null) return " Received null";
  if (value === undefined) return " Received undefined";
  if (typeof value === "string") return ` Received type string ('${value}')`;
  if (typeof value === "boolean") return ` Received type boolean (${value})`;
  if (typeof value === "number") return ` Received type number (${value})`;
  if (typeof value === "boolean") return ` Received type boolean (${value})`;
  if (typeof value === "bigint") return ` Received type bigint (${value}n)`;
  if (typeof value === "symbol") {
    return ` Received type symbol (${String(value)})`;
  }
  if (Array.isArray(value)) return " Received an instance of Array";
  return ` Received an instance of ${value?.constructor?.name || "Object"}`;
};
const __quenchDgramBufferError = (type, code, message) => {
  const syscall = `uv_${type}_buffer_size`;
  const error = new Error(
    `Could not get or set buffer size: ${syscall} returned ${code} (${message})`
  );
  error.name = "SystemError";
  error.code = "ERR_SOCKET_BUFFER_SIZE";
  error.info = { errno: undefined, code, message, syscall };
  let errorErrno;
  Object.defineProperty(error, "errno", {
    enumerable: true,
    get: () => errorErrno,
    set: (value) => {
      errorErrno = value;
    }
  });
  let errorSyscall = syscall;
  Object.defineProperty(error, "syscall", {
    enumerable: true,
    get: () => errorSyscall,
    set: (value) => {
      errorSyscall = value;
    }
  });
  return error;
};
const __quenchDgramBind = (socket, type, port, address, callback) => {
  if (socket._bound) {
    throw Object.assign(new Error("Socket is already bound"), {
      code: "ERR_SOCKET_ALREADY_BOUND"
    });
  }
  if (typeof port === "function") {
    callback = port;
    port = 0;
  }
  if (typeof address === "function") callback = address;
  const localAddress =
    type === "udp6"
      ? address === "::" || address === "::1"
      : address === "0.0.0.0" || address === "127.0.0.1";
  if (typeof address === "string" && !localAddress) {
    const error = Object.assign(new Error(`bind EADDRNOTAVAIL ${address}`), {
      code: "EADDRNOTAVAIL",
      address,
      syscall: "bind"
    });
    queueMicrotask(() => socket.emit("error", error));
    return socket;
  }
  const resolvedPort =
    typeof port === "number" && port > 0 ? port : __quenchDgramNextPort++;
  if (__quenchDgramBoundPorts.has(resolvedPort) && !socket._reusePort) {
    const error = Object.assign(new Error("bind EADDRINUSE"), {
      code: "EADDRINUSE",
      syscall: "bind"
    });
    queueMicrotask(() => socket.emit("error", error));
    return socket;
  }
  socket._bound = true;
  __quenchDgramSockets.add(socket);
  socket._bindPending = true;
  __quenchDgramBoundPorts.add(resolvedPort);
  __quenchDgramClosedPorts.delete(resolvedPort);
  globalThis.__quenchDgramActiveFds.add(resolvedPort);
  socket[__quenchDgramStateSymbol].handle.fd = resolvedPort;
  globalThis.__quenchDgramUdpFds.add(resolvedPort);
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
  if (message === undefined) {
    throw Object.assign(
      new TypeError(
        'The "buffer" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received undefined'
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (
    typeof message !== "string" &&
    !(message instanceof NodeBuffer) &&
    !Array.isArray(message) &&
    !ArrayBuffer.isView(message)
  ) {
    throw Object.assign(
      new TypeError(
        `The "buffer" argument must be of type string or an instance of Buffer, TypedArray, or DataView.${__quenchDgramTypeDetail(
          message
        )}`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  const wasUnbound = !socket._bound && !socket._connected;
  const implicitlyBound = wasUnbound;
  if (implicitlyBound) {
    socket._implicitSend = true;
    __quenchDgramBind(
      socket,
      socket.type,
      0,
      socket.type === "udp6" ? "::" : "0.0.0.0"
    );
  }
  const hasOffset =
    (!socket._connected &&
      args.length >= 3 &&
      typeof args[0] === "number" &&
      typeof args[1] === "number") ||
    (socket._connected && args.length >= 2);
  const addressIndex = hasOffset
    ? args.length >= 4
      ? 3
      : -1
    : args.length >= 2
      ? 1
      : -1;
  const address =
    addressIndex < 0 || typeof args[addressIndex] === "function"
      ? undefined
      : args[addressIndex];
  const callback = args.at(-1);
  if (
    address !== undefined &&
    address !== null &&
    address !== "" &&
    typeof address !== "function" &&
    typeof address !== "string"
  ) {
    throw Object.assign(
      new TypeError(
        `The "address" argument must be of type string.${__quenchDgramTypeDetail(
          address
        )}`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (
    address &&
    socket._sendBlockList?.check?.(
      address,
      socket.type === "udp6" ? "ipv6" : "ipv4"
    )
  ) {
    const error = Object.assign(new Error("IP is blocked"), {
      code: "ERR_IP_BLOCKED"
    });
    queueMicrotask(() => callback?.(error));
    return socket;
  }
  if (address === "localhost" && !socket._skipDgramLookup) {
    socket._skipDgramLookup = true;
    socket[__quenchDgramStateSymbol].handle.lookup(address, (error) => {
      if (error) {
        socket._skipDgramLookup = false;
        queueMicrotask(() => {
          if (typeof callback === "function") callback(error);
          else socket.emit("error", error);
        });
        return;
      }
      __quenchDgramSend(socket, message, ...args);
      socket._skipDgramLookup = false;
    });
    return socket;
  }
  const isNumericAddress =
    !address ||
    address === "localhost" ||
    /^\d{1,3}(?:\.\d{1,3}){3}$/.test(address) ||
    address.includes(":");
  if (!isNumericAddress) {
    const error = Object.assign(new Error(`getaddrinfo ENOTFOUND ${address}`), {
      code: "ENOTFOUND",
      syscall: "getaddrinfo",
      hostname: address
    });
    queueMicrotask(() => {
      if (typeof callback === "function") callback(error);
      else socket.emit("error", error);
    });
    return socket;
  }
  if (
    Array.isArray(message) &&
    message.some(
      (chunk) =>
        typeof chunk !== "string" &&
        !(chunk instanceof NodeBuffer) &&
        !ArrayBuffer.isView(chunk) &&
        !(chunk instanceof ArrayBuffer)
    )
  ) {
    throw Object.assign(
      new TypeError(
        'The "buffer list arguments" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received an instance of Array'
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  const destinationPort = socket._connected
    ? socket._remote?.port
    : hasOffset
      ? args.length >= 3
        ? args[2]
        : undefined
      : args[0];
  if (
    !socket._connected &&
    destinationPort !== undefined &&
    (!Number.isInteger(destinationPort) ||
      destinationPort <= 0 ||
      destinationPort >= 65536)
  ) {
    throw Object.assign(new RangeError("Port should be > 0 and < 65536"), {
      code: "ERR_SOCKET_BAD_PORT"
    });
  }
  const payload =
    typeof message === "string"
      ? NodeBuffer.from(message)
      : Array.isArray(message)
        ? NodeBuffer.concat(
            message.map((chunk) =>
              typeof chunk === "string" ? NodeBuffer.from(chunk) : chunk
            )
          )
        : message;
  const bytePayload =
    payload instanceof NodeBuffer
      ? payload
      : ArrayBuffer.isView(payload)
        ? NodeBuffer.from(
            payload.buffer,
            payload.byteOffset,
            payload.byteLength
          )
        : payload instanceof ArrayBuffer
          ? NodeBuffer.from(payload)
          : NodeBuffer.from(payload);
  const offset = hasOffset ? args[0] : 0;
  const length = hasOffset ? args[1] : payload.byteLength;
  if (
    socket._connected &&
    args.length >= 3 &&
    (typeof args[2] !== "function" ||
      (typeof args[0] === "number" && typeof args[1] === "string"))
  ) {
    throw Object.assign(new Error("Already connected"), {
      code: "ERR_SOCKET_DGRAM_IS_CONNECTED"
    });
  }
  if (
    hasOffset &&
    (!Number.isInteger(offset) || offset < 0 || offset > bytePayload.byteLength)
  ) {
    throw Object.assign(
      new RangeError('"offset" is outside of buffer bounds'),
      { code: "ERR_BUFFER_OUT_OF_BOUNDS" }
    );
  }
  if (
    hasOffset &&
    (!Number.isInteger(length) ||
      length < 0 ||
      offset + length > bytePayload.byteLength)
  ) {
    throw Object.assign(
      new RangeError('"length" is outside of buffer bounds'),
      { code: "ERR_BUFFER_OUT_OF_BOUNDS" }
    );
  }
  const deliveredPayload = hasOffset
    ? bytePayload.subarray(offset, offset + length)
    : bytePayload;
  if (deliveredPayload.byteLength > 65507) {
    const error = Object.assign(
      new Error(`send EMSGSIZE ${address || "127.0.0.1"}:${destinationPort}`),
      {
        code: "EMSGSIZE",
        syscall: "send",
        address: address || "127.0.0.1",
        port: destinationPort
      }
    );
    queueMicrotask(() => {
      if (typeof callback === "function") callback(error);
      else socket.emit("error", error);
    });
    return socket;
  }
  const sendResult = socket[__quenchDgramStateSymbol].handle.send?.();
  if (sendResult) {
    const error = Object.assign(
      new Error(`send UNKNOWN ${address || "127.0.0.1"}:${destinationPort}`),
      {
        code: "UNKNOWN",
        errno: sendResult,
        syscall: "send",
        address: address || "127.0.0.1",
        port: destinationPort
      }
    );
    queueMicrotask(() => {
      if (typeof callback === "function") callback(error);
      else socket.emit("error", error);
    });
    return socket;
  }
  if (socket._connected) {
    socket._sendQueueSize = (socket._sendQueueSize || 0) + length;
    socket._sendQueueCount = (socket._sendQueueCount || 0) + 1;
  }
  queueMicrotask(() => {
    const target = [...__quenchDgramSockets].find(
      (candidate) =>
        candidate._bound && candidate._address?.port === destinationPort
    );
    const sourceAddress = {
      ...(socket._address || {
        address: socket.type === "udp6" ? "::1" : "127.0.0.1",
        family: socket.type === "udp6" ? "IPv6" : "IPv4",
        port: 0
      }),
      address:
        socket._address?.address === "0.0.0.0"
          ? "127.0.0.1"
          : socket._address?.address === "::"
            ? "::1"
            : socket._address?.address ||
              (socket.type === "udp6" ? "::1" : "127.0.0.1")
    };
    const blocked = target?._receiveBlockList?.check?.(
      sourceAddress.address,
      socket.type === "udp6" ? "ipv6" : "ipv4"
    );
    if (!blocked) {
      target?.emit("message", deliveredPayload, {
        ...sourceAddress,
        size: deliveredPayload.byteLength
      });
    }
    if (
      (target ||
        (socket._implicitSend &&
          !__quenchDgramClosedPorts.has(destinationPort))) &&
      typeof callback === "function"
    ) {
      callback(null, length);
      socket._implicitSend = false;
    }
  });
  return socket;
};
const __quenchDgramSendTo = (socket, message, ...args) => {
  if (args[0] === undefined) {
    throw Object.assign(
      new TypeError(
        'The "offset" argument must be of type number. Received undefined'
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (typeof args[0] !== "number") {
    throw Object.assign(
      new TypeError(
        `The "offset" argument must be of type number. Received type string ('${
          args[0]
        }')`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (typeof args[1] !== "number") {
    throw Object.assign(
      new TypeError(
        `The "length" argument must be of type number. Received type string ('${
          args[1]
        }')`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (typeof args[3] !== "string") {
    throw Object.assign(
      new TypeError(
        `The "address" argument must be of type string. Received type boolean (${
          args[3]
        })`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (typeof args[2] !== "number") {
    throw Object.assign(
      new TypeError(
        `The "port" argument must be of type number. Received type boolean (${
          args[2]
        })`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  return __quenchDgramSend(socket, message, ...args);
};
const __quenchDgramConnect = (socket, port, address, callback) => {
  if (typeof address === "function") {
    callback = address;
    address = "127.0.0.1";
  }
  address ??= socket.type === "udp6" ? "::1" : "127.0.0.1";
  if (!Number.isInteger(port) || port <= 0 || port >= 65536) {
    throw Object.assign(new RangeError("Port should be > 0 and < 65536"), {
      code: "ERR_SOCKET_BAD_PORT"
    });
  }
  if (socket._connected || socket._connecting) {
    throw Object.assign(new Error("Already connected"), {
      code: "ERR_SOCKET_DGRAM_IS_CONNECTED"
    });
  }
  if (
    socket._sendBlockList?.check?.(
      address,
      socket.type === "udp6" ? "ipv6" : "ipv4"
    )
  ) {
    const error = Object.assign(new Error("IP is blocked"), {
      code: "ERR_IP_BLOCKED"
    });
    queueMicrotask(() => callback?.(error));
    return socket;
  }
  socket._connecting = true;
  socket._remote = { address, port };
  if (typeof callback === "function") socket.once("connect", callback);
  queueMicrotask(() => {
    socket._connecting = false;
    socket._connected = true;
    socket.emit("connect");
  });
  return socket;
};
const __quenchDgramDisconnect = (socket) => {
  if (!socket._connected) {
    throw Object.assign(new Error("Not connected"), {
      code: "ERR_SOCKET_DGRAM_NOT_CONNECTED"
    });
  }
  socket._connected = false;
  socket._remote = undefined;
  return socket;
};
const __quenchDgramRemoteAddress = (socket) => {
  if (!socket._connected) {
    throw Object.assign(new Error("Not connected"), {
      code: "ERR_SOCKET_DGRAM_NOT_CONNECTED"
    });
  }
  return {
    ...socket._remote,
    family: socket.type === "udp6" ? "IPv6" : "IPv4"
  };
};
const __quenchDgramClose = (socket, callback) => {
  if (socket._closed) return socket;
  socket._closed = true;
  socket._bound = false;
  __quenchDgramSockets.delete(socket);
  if (socket[__quenchDgramStateSymbol]?.handle?.fd !== undefined) {
    globalThis.__quenchDgramActiveFds.delete(
      socket[__quenchDgramStateSymbol].handle.fd
    );
  }
  if (socket._address) {
    __quenchDgramBoundPorts.delete(socket._address.port);
    __quenchDgramClosedPorts.add(socket._address.port);
  }
  if (typeof callback === "function") callback();
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
          code: "ERR_SOCKET_ALREADY_BOUND"
        });
      }
      if (typeof address === "function") {
        callback = address;
        address = undefined;
      }
      if (port && typeof port === "object" && port.fd !== undefined) {
        if (globalThis.__quenchDgramActiveFds.has(port.fd)) {
          throw Object.assign(new Error("open EEXIST"), {
            code: "EEXIST"
          });
        }
        if (!globalThis.__quenchDgramUdpFds.has(port.fd)) {
          throw Object.assign(new TypeError("Unsupported fd type: TCP"), {
            code: "ERR_INVALID_FD_TYPE"
          });
        }
        socket._bound = true;
        __quenchDgramSockets.add(socket);
        socket[__quenchDgramStateSymbol].handle.fd = port.fd;
        globalThis.__quenchDgramActiveFds.add(port.fd);
        socket._address = {
          ...(globalThis.__quenchDgramUdpHandleInfo.get(port.fd) || {}),
          address:
            port.address ||
            globalThis.__quenchDgramUdpHandleInfo.get(port.fd)?.address ||
            (type === "udp6" ? "::" : "0.0.0.0"),
          family:
            globalThis.__quenchDgramUdpHandleInfo.get(port.fd)?.family ||
            (type === "udp6" ? "IPv6" : "IPv4"),
          port:
            port.port ||
            globalThis.__quenchDgramUdpHandleInfo.get(port.fd)?.port ||
            __quenchDgramNextPort++
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
      const isIpv4Literal =
        typeof lookupAddress === "string" &&
        /^\d{1,3}(?:\.\d{1,3}){3}$/.test(lookupAddress);
      const isIpv6Literal =
        typeof lookupAddress === "string" && lookupAddress.includes(":");
      const isHostname =
        typeof lookupAddress === "string" &&
        lookupAddress !== "localhost" &&
        (type === "udp4" ? !isIpv4Literal : !isIpv6Literal);
      const lookup =
        socket._lookup ||
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
          code: "ERR_SOCKET_ALREADY_BOUND"
        });
      }
      if (
        options !== undefined &&
        (options === null || typeof options !== "object")
      ) {
        throw Object.assign(
          new TypeError('The "options" argument must be of type object'),
          {
            code: "ERR_INVALID_ARG_TYPE"
          }
        );
      }
      const config = options;
      const port = config?.port ?? 0;
      const resolvedPort = port || __quenchDgramNextPort++;
      const address = config?.address || (type === "udp6" ? "::" : "0.0.0.0");
      if (!Number.isInteger(port) || port < 0 || port > 65535) {
        throw Object.assign(new RangeError("Port should be >= 0 and < 65536"), {
          code: "ERR_SOCKET_BAD_PORT"
        });
      }
      if (typeof address !== "string") {
        throw Object.assign(
          new TypeError('The "address" argument must be of type string'),
          {
            code: "ERR_INVALID_ARG_TYPE"
          }
        );
      }
      if (address === "localhost") {
        throw Object.assign(new TypeError("Invalid IP address"), {
          code: "ERR_INVALID_ARG_VALUE"
        });
      }
      if (
        options?.sendBlockList?.check?.(
          address,
          type === "udp6" ? "ipv6" : "ipv4"
        )
      ) {
        throw Object.assign(new Error("IP is blocked"), {
          code: "ERR_IP_BLOCKED"
        });
      }
      if (__quenchDgramBoundPorts.has(resolvedPort)) {
        throw Object.assign(new Error("bind EADDRINUSE"), {
          code: "EADDRINUSE",
          syscall: "bind"
        });
      }
      socket._bound = true;
      __quenchDgramSockets.add(socket);
      __quenchDgramBoundPorts.add(resolvedPort);
      __quenchDgramClosedPorts.delete(resolvedPort);
      socket._address = {
        address,
        family: type === "udp6" ? "IPv6" : "IPv4",
        port: resolvedPort
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
          code: "ERR_SOCKET_ALREADY_BOUND"
        });
      }
      if (!Number.isInteger(port) || port <= 0 || port >= 65536) {
        throw Object.assign(new RangeError("Port should be > 0 and < 65536"), {
          code: "ERR_SOCKET_BAD_PORT"
        });
      }
      if (typeof address !== "string") {
        throw Object.assign(
          new TypeError('The "address" argument must be of type string'),
          {
            code: "ERR_INVALID_ARG_TYPE"
          }
        );
      }
      if (address === "localhost") {
        throw Object.assign(new TypeError("Invalid IP address"), {
          code: "ERR_INVALID_ARG_VALUE"
        });
      }
      if (socket._connected) {
        throw Object.assign(new Error("Already connected"), {
          code: "ERR_SOCKET_DGRAM_IS_CONNECTED"
        });
      }
      if (
        options?.sendBlockList?.check?.(
          address,
          type === "udp6" ? "ipv6" : "ipv4"
        )
      ) {
        throw Object.assign(new Error("IP is blocked"), {
          code: "ERR_IP_BLOCKED"
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
          port: localPort
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
            code: "ERR_SOCKET_BAD_BUFFER_SIZE"
          }
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
            code: "ERR_SOCKET_BAD_BUFFER_SIZE"
          }
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
            `The "ttl" argument must be of type number. Received type string ('${value}')`
          ),
          { code: "ERR_INVALID_ARG_TYPE" }
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
            `The "ttl" argument must be of type number. Received type string ('${value}')`
          ),
          { code: "ERR_INVALID_ARG_TYPE" }
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
          listener.__quenchOriginalListener !== callback
      );
      return socket;
    },
    once: (event, callback) =>
      __quenchDgramOnce(socket, listeners, event, callback),
    emit: (event, ...args) => __quenchDgramEmit(socket, listeners, event, args),
    ref: () => socket,
    unref: () => socket
  };
  Object.assign(socket, __quenchDgramMembershipMethods(socket));
  socket[__quenchDgramStateSymbol] = {
    handle: {
      fd: 0,
      lookup(address, familyOrCallback, maybeCallback) {
        const callback =
          typeof familyOrCallback === "function"
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
          errno: status
        });
        socket.emit("error", error);
      }
    }
  };
  if (
    options?.signal !== undefined &&
    (!options.signal || typeof options.signal.addEventListener !== "function")
  ) {
    throw Object.assign(
      new TypeError('The "signal" option must be an AbortSignal'),
      { code: "ERR_INVALID_ARG_TYPE" }
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
    ) {
      return __quenchDgramValidateType(type);
    }
    const requested =
      typeof type === "string" ? type : type?.type || options?.type;
    const config = typeof type === "object" ? type : options;
    if (config?.lookup !== undefined && typeof config.lookup !== "function") {
      throw Object.assign(
        new TypeError(
          `The "lookup" argument must be of type function.${__quenchDgramTypeDetail(
            config.lookup
          )}`
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    for (const name of ["recvBufferSize", "sendBufferSize"]) {
      if (config?.[name] !== undefined && typeof config[name] !== "number") {
        throw Object.assign(
          new TypeError(`The "${name}" option must be a number`),
          {
            code: "ERR_INVALID_ARG_TYPE"
          }
        );
      }
    }
    return __quenchDgramSocket(__quenchDgramValidateType(requested), config);
  }
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
          }
        }
      : __quenchOriginalRequireWithDgram(specifier);
