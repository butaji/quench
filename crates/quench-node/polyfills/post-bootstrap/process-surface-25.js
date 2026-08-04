{
  if (globalThis.process) {
    const originalBinding = globalThis.process.binding;
    const allowedInternalBindings = new Set([
      "buffer",
      "cares_wrap",
      "constants",
      "contextify",
      "fs",
      "fs_event_wrap",
      "icu",
      "inspector",
      "js_stream",
      "natives",
      "os",
      "pipe_wrap",
      "spawn_sync",
      "stream_wrap",
      "tcp_wrap",
      "tls_wrap",
      "tty_wrap",
      "udp_wrap",
      "uv",
      "zlib"
    ]);
    globalThis.process.binding = (name) => {
      if (name === "util" && globalThis.__nodeUtil?.types) {
        const types = globalThis.__nodeUtil.types;
        return {
          isAnyArrayBuffer: types.isAnyArrayBuffer,
          isArrayBuffer: types.isArrayBuffer,
          isArrayBufferView: types.isArrayBufferView,
          isAsyncFunction: types.isAsyncFunction,
          isDataView: types.isDataView,
          isDate: types.isDate,
          isExternal: types.isExternal,
          isMap: types.isMap,
          isMapIterator: types.isMapIterator,
          isNativeError: types.isNativeError,
          isPromise: types.isPromise,
          isRegExp: types.isRegExp,
          isSet: types.isSet,
          isSetIterator: types.isSetIterator,
          isTypedArray: types.isTypedArray,
          isUint8Array: types.isUint8Array
        };
      }
      if (allowedInternalBindings.has(name)) return {};
      return originalBinding(name);
    };
    globalThis.process._linkedBinding ||= () => ({});
    globalThis.process.dlopen ||= () => undefined;
  }
}
