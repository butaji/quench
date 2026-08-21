//! Polyfill: `dgram`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchDgramBind = (socket, type, port, address, callback) => {
  if (socket._bound) {
    throw Object.assign(new Error("Socket is already bound"), {
      code: "ERR_SOCKET_ALREADY_BOUND",
    });
  }
  if (typeof port === "function") {
    callback = port;
    port = 0;
  }
  if (typeof address === "function") callback = address;
  const localAddress = type === "udp6"
    ? address === "::" || address === "::1"
    : address === "0.0.0.0" || address === "127.0.0.1";
  if (typeof address === "string" && !localAddress) {
    const error = Object.assign(new Error(`bind EADDRNOTAVAIL ${address}`), {
      code: "EADDRNOTAVAIL",
      address,
      syscall: "bind",
    });
    queueMicrotask(() => socket.emit("error", error));
    return socket;
  }
  const resolvedPort = typeof port === "number" && port > 0
    ? port
    : __quenchDgramNextPort++;
  if (__quenchDgramBoundPorts.has(resolvedPort) && !socket._reusePort) {
    const error = Object.assign(new Error("bind EADDRINUSE"), {
      code: "EADDRINUSE",
      syscall: "bind",
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
    address: typeof address === "string"
      ? address
      : type === "udp6"
      ? "::"
      : "0.0.0.0",
    family: type === "udp6" ? "IPv6" : "IPv4",
    port: resolvedPort,
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
        'The "buffer" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received undefined',
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
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
        `The "buffer" argument must be of type string or an instance of Buffer, TypedArray, or DataView.${
          __quenchDgramTypeDetail(
            message,
          )
        }`,
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
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
      socket.type === "udp6" ? "::" : "0.0.0.0",
    );
  }
  const hasOffset = (!socket._connected &&
    args.length >= 3 &&
    typeof args[0] === "number" &&
    typeof args[1] === "number") ||
    (socket._connected && args.length >= 2);
  const addressIndex = hasOffset
    ? args.length >= 4 ? 3 : -1
    : args.length >= 2
    ? 1
    : -1;
  const address = addressIndex < 0 || typeof args[addressIndex] === "function"
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
        `The "address" argument must be of type string.${
          __quenchDgramTypeDetail(
            address,
          )
        }`,
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (
    address &&
    socket._sendBlockList?.check?.(
      address,
      socket.type === "udp6" ? "ipv6" : "ipv4",
    )
  ) {
    const error = Object.assign(new Error("IP is blocked"), {
      code: "ERR_IP_BLOCKED",
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
  const isNumericAddress = !address ||
    address === "localhost" ||
    /^\d{1,3}(?:\.\d{1,3}){3}$/.test(address) ||
    address.includes(":");
  if (!isNumericAddress) {
    const error = Object.assign(new Error(`getaddrinfo ENOTFOUND ${address}`), {
      code: "ENOTFOUND",
      syscall: "getaddrinfo",
      hostname: address,
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
        !(chunk instanceof ArrayBuffer),
    )
  ) {
    throw Object.assign(
      new TypeError(
        'The "buffer list arguments" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received an instance of Array',
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  const destinationPort = socket._connected
    ? socket._remote?.port
    : hasOffset
    ? args.length >= 3 ? args[2] : undefined
    : args[0];
  if (
    !socket._connected &&
    destinationPort !== undefined &&
    (!Number.isInteger(destinationPort) ||
      destinationPort <= 0 ||
      destinationPort >= 65536)
  ) {
    throw Object.assign(new RangeError("Port should be > 0 and < 65536"), {
      code: "ERR_SOCKET_BAD_PORT",
    });
  }
  const payload = typeof message === "string"
    ? NodeBuffer.from(message)
    : Array.isArray(message)
    ? NodeBuffer.concat(
      message.map((chunk) =>
        typeof chunk === "string" ? NodeBuffer.from(chunk) : chunk
      ),
    )
    : message;
  const bytePayload = payload instanceof NodeBuffer
    ? payload
    : ArrayBuffer.isView(payload)
    ? NodeBuffer.from(
      payload.buffer,
      payload.byteOffset,
      payload.byteLength,
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
      code: "ERR_SOCKET_DGRAM_IS_CONNECTED",
    });
  }
  if (
    hasOffset &&
    (!Number.isInteger(offset) || offset < 0 || offset > bytePayload.byteLength)
  ) {
    throw Object.assign(
      new RangeError('"offset" is outside of buffer bounds'),
      { code: "ERR_BUFFER_OUT_OF_BOUNDS" },
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
      { code: "ERR_BUFFER_OUT_OF_BOUNDS" },
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
        port: destinationPort,
      },
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
        port: destinationPort,
      },
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
        candidate._bound && candidate._address?.port === destinationPort,
    );
    const sourceAddress = {
      ...(socket._address || {
        address: socket.type === "udp6" ? "::1" : "127.0.0.1",
        family: socket.type === "udp6" ? "IPv6" : "IPv4",
        port: 0,
      }),
      address: socket._address?.address === "0.0.0.0"
        ? "127.0.0.1"
        : socket._address?.address === "::"
        ? "::1"
        : socket._address?.address ||
          (socket.type === "udp6" ? "::1" : "127.0.0.1"),
    };
    const blocked = target?._receiveBlockList?.check?.(
      sourceAddress.address,
      socket.type === "udp6" ? "ipv6" : "ipv4",
    );
    if (!blocked) {
      target?.emit("message", deliveredPayload, {
        ...sourceAddress,
        size: deliveredPayload.byteLength,
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
        'The "offset" argument must be of type number. Received undefined',
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (typeof args[0] !== "number") {
    throw Object.assign(
      new TypeError(
        `The "offset" argument must be of type number. Received type string ('${
          args[0]
        }')`,
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (typeof args[1] !== "number") {
    throw Object.assign(
      new TypeError(
        `The "length" argument must be of type number. Received type string ('${
          args[1]
        }')`,
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (typeof args[3] !== "string") {
    throw Object.assign(
      new TypeError(
        `The "address" argument must be of type string. Received type boolean (${
          args[3]
        })`,
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
    );
  }
  if (typeof args[2] !== "number") {
    throw Object.assign(
      new TypeError(
        `The "port" argument must be of type number. Received type boolean (${
          args[2]
        })`,
      ),
      { code: "ERR_INVALID_ARG_TYPE" },
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
      code: "ERR_SOCKET_BAD_PORT",
    });
  }
  if (socket._connected || socket._connecting) {
    throw Object.assign(new Error("Already connected"), {
      code: "ERR_SOCKET_DGRAM_IS_CONNECTED",
    });
  }
  if (
    socket._sendBlockList?.check?.(
      address,
      socket.type === "udp6" ? "ipv6" : "ipv4",
    )
  ) {
    const error = Object.assign(new Error("IP is blocked"), {
      code: "ERR_IP_BLOCKED",
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
      code: "ERR_SOCKET_DGRAM_NOT_CONNECTED",
    });
  }
  socket._connected = false;
  socket._remote = undefined;
  return socket;
};
const __quenchDgramRemoteAddress = (socket) => {
  if (!socket._connected) {
    throw Object.assign(new Error("Not connected"), {
      code: "ERR_SOCKET_DGRAM_NOT_CONNECTED",
    });
  }
  return {
    ...socket._remote,
    family: socket.type === "udp6" ? "IPv6" : "IPv4",
  };
};
const __quenchDgramClose = (socket, callback) => {
  if (socket._closed) return socket;
  socket._closed = true;
  socket._bound = false;
  __quenchDgramSockets.delete(socket);
  if (socket[__quenchDgramStateSymbol]?.handle?.fd !== undefined) {
    globalThis.__quenchDgramActiveFds.delete(
      socket[__quenchDgramStateSymbol].handle.fd,
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
  if (!socket._bound) {
    throw Object.assign(new Error("getsockname EBADF"), {
      code: "EBADF",
      errno: -9,
      syscall: "getsockname",
    });
  }
  return (
    socket._address || {
      address: "0.0.0.0",
      family: type === "udp6" ? "IPv6" : "IPv4",
      port: 0,
    }
  );
};
const __quenchDgramOn = (socket, listeners, event, callback) => {
  (listeners[event] ||= []).push(callback);
  return socket;
};
"#);
