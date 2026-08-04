const __quenchOriginalRequireWithZlibStreams = globalThis.require;
const __quenchZlibTransform = (compress, method) => {
  const listeners = {};
  const stream = {
    on(event, callback) {
      (listeners[event] ||= []).push(callback);
      return stream;
    },
    emit(event, ...args) {
      for (const callback of listeners[event] || []) callback(...args);
      return stream;
    },
    write(input) {
      stream.emit("data", method(input));
      return true;
    },
    end(input) {
      if (input !== undefined) stream.write(input);
      queueMicrotask(() => stream.emit("end"));
      return stream;
    },
    pipe(destination) {
      stream.on("data", (chunk) => destination.write(chunk));
      stream.on("end", () => destination.end());
      return destination;
    },
    readable: true,
    writable: true
  };
  return stream;
};
globalThis.require = (specifier) => {
  const module = __quenchOriginalRequireWithZlibStreams(specifier);
  if (String(specifier).replace(/^node:/, "") !== "zlib") return module;
  return Object.assign({}, module, {
    createGzip: (options) => __quenchZlibTransform(options, module.gzipSync),
    createGunzip: (options) =>
      __quenchZlibTransform(options, module.gunzipSync),
    createDeflate: (options) =>
      __quenchZlibTransform(options, module.deflateSync),
    createInflate: (options) =>
      __quenchZlibTransform(options, module.inflateSync)
  });
};
