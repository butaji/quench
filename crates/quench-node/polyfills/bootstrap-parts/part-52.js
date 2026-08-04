const __quenchOriginalRequireWithDgram = globalThis.require;
const __quenchDgramBind = (socket, type, port, address, callback) => {
  if (typeof address === "function") callback = address;
  socket._address = {
    address: typeof address === "string" ? address : "0.0.0.0",
    family: type === "udp6" ? "IPv6" : "IPv4",
    port: typeof port === "number" ? port : 0
  };
  callback?.();
  queueMicrotask(() => socket.emit("listening"));
  return socket;
};
const __quenchDgramSend = (socket, message, address, callback) => {
  if (typeof address === "function") callback = address;
  queueMicrotask(() => callback?.(null));
  return socket;
};
const __quenchDgramClose = (socket, callback) => {
  callback?.();
  queueMicrotask(() => socket.emit("close"));
  return socket;
};
const __quenchDgramAddress = (socket, type) =>
  socket._address || {
    address: "0.0.0.0",
    family: type === "udp6" ? "IPv6" : "IPv4",
    port: 0
  };
const __quenchDgramOn = (socket, listeners, event, callback) => {
  (listeners[event] ||= []).push(callback);
  return socket;
};
const __quenchDgramEmit = (socket, listeners, event, args) => {
  for (const callback of listeners[event] || []) callback(...args);
  return socket;
};
const __quenchDgramSocket = (type = "udp4") => {
  const listeners = {};
  const socket = {
    type,
    bind: (port, address, callback) =>
      __quenchDgramBind(socket, type, port, address, callback),
    send: (message, port, address, callback) =>
      __quenchDgramSend(socket, message, address, callback),
    close: (callback) => __quenchDgramClose(socket, callback),
    address: () => __quenchDgramAddress(socket, type),
    on: (event, callback) =>
      __quenchDgramOn(socket, listeners, event, callback),
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
