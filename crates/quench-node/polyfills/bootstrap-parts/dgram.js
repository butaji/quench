const __quenchOriginalRequireWithDgram = globalThis.require;
const __quenchDgramBind = (socket, type, port, address, callback) => {
  if (socket._bound)
    throw Object.assign(new Error("Socket is already bound"), {
      code: "ERR_SOCKET_ALREADY_BOUND"
    });
  if (typeof address === "function") callback = address;
  socket._bound = true;
  socket._address = {
    address: typeof address === "string" ? address : "0.0.0.0",
    family: type === "udp6" ? "IPv6" : "IPv4",
    port: typeof port === "number" && port > 0 ? port : 40000
  };
  queueMicrotask(() => callback?.());
  queueMicrotask(() => socket.emit("listening"));
  return socket;
};
const __quenchDgramSend = (socket, message, ...args) => {
  const callback = args.at(-1);
  const hasOffset = args.length >= 5 || (socket._connected && args.length >= 3);
  const payload = Array.isArray(message) ? NodeBuffer.concat(message) : message;
  const length = hasOffset ? args[1] : payload.byteLength;
  queueMicrotask(() => {
    socket.emit("message", payload, socket.address());
    if (typeof callback === "function") callback(null, length);
  });
  return socket;
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
const __quenchDgramSocket = (type = "udp4") => {
  const listeners = {};
  const socket = {
    type,
    bind: (port, address, callback) =>
      __quenchDgramBind(socket, type, port, address, callback),
    send: (message, ...args) => __quenchDgramSend(socket, message, ...args),
    connect: (port, address, callback) =>
      __quenchDgramConnect(socket, port, address, callback),
    disconnect: () => __quenchDgramDisconnect(socket),
    remoteAddress: () => __quenchDgramRemoteAddress(socket),
    close: (callback) => __quenchDgramClose(socket, callback),
    address: () => __quenchDgramAddress(socket, type),
    on: (event, callback) =>
      __quenchDgramOn(socket, listeners, event, callback),
    once: (event, callback) =>
      __quenchDgramOnce(socket, listeners, event, callback),
    emit: (event, ...args) => __quenchDgramEmit(socket, listeners, event, args),
    unref: () => socket
  };
  return socket;
};
const __quenchDgram = {
  createSocket: (type, options) =>
    __quenchDgramSocket(
      typeof type === "string" ? type : options?.type || "udp4"
    )
};
globalThis.require = (specifier) =>
  String(specifier).replace(/^node:/, "") === "dgram"
    ? __quenchDgram
    : __quenchOriginalRequireWithDgram(specifier);
