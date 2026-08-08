const __quenchOriginalRequireWithZlibIter = globalThis.require;
const __quenchZlibIterTransform = (method) =>
  async function* (source) {
    for await (const chunk of source) yield method(chunk);
  };
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "zlib/iter") {
    const zlib = __quenchOriginalRequireWithZlibIter("zlib");
    return {
      compressGzip: () => __quenchZlibIterTransform(zlib.gzipSync),
      decompressGzip: () => __quenchZlibIterTransform(zlib.gunzipSync)
    };
  }
  return __quenchOriginalRequireWithZlibIter(specifier);
};
