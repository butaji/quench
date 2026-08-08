const __quenchOriginalRequireWithZlibIter = globalThis.require;
const __quenchZlibIterInput = (chunk) => {
  if (!Array.isArray(chunk)) return Buffer.from(chunk);
  return Buffer.concat(chunk.map((part) => Buffer.from(part)));
};
const __quenchZlibIterTransform = (method) =>
  async function* (source) {
    for await (const chunk of source)
      yield method(__quenchZlibIterInput(chunk));
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
