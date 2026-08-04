const __quenchOriginalRequireWithStreamPromises = globalThis.require;
const __quenchStreamPromises = {
  pipeline: (...streams) => {
    const destination = streams.pop();
    for (const source of streams) source.pipe(destination);
    return new Promise((resolve, reject) => {
      destination.on("end", resolve);
      destination.on("error", reject);
    });
  },
  finished: (stream) =>
    new Promise((resolve, reject) => {
      stream.on("end", resolve);
      stream.on("error", reject);
    })
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "stream/promises")
    return __quenchStreamPromises;
  return __quenchOriginalRequireWithStreamPromises(specifier);
};
