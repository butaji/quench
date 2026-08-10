//! Polyfill: `iterators`

pub const JS: &str = r#"const __quenchOriginalRequireWithZlibIter = globalThis.require;
const __quenchZlibIterInput = (chunk) => {
  if (!Array.isArray(chunk)) return Buffer.from(chunk);
  return Buffer.concat(chunk.map((part) => Buffer.from(part)));
};
const __quenchZlibIterTransform = (method) =>
  async function* (source) {
    for await (const chunk of source) {
      yield method(__quenchZlibIterInput(chunk));
    }
  };
const __quenchZlibSyncIterTransform = (method) => (chunk) =>
  chunk === null ? null : method(__quenchZlibIterInput(chunk));
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "zlib/iter") {
    const zlib = __quenchOriginalRequireWithZlibIter("zlib");
    return {
      compressGzip: () => __quenchZlibIterTransform(zlib.gzipSync),
      decompressGzip: () => __quenchZlibIterTransform(zlib.gunzipSync),
      compressGzipSync: () => __quenchZlibSyncIterTransform(zlib.gzipSync),
      decompressGzipSync: () => __quenchZlibSyncIterTransform(zlib.gunzipSync),
      compressDeflateSync: () =>
        __quenchZlibSyncIterTransform(zlib.deflateSync),
      decompressDeflateSync: () =>
        __quenchZlibSyncIterTransform(zlib.inflateSync),
      compressBrotliSync: () =>
        __quenchZlibSyncIterTransform(zlib.brotliCompressSync),
      decompressBrotliSync: () =>
        __quenchZlibSyncIterTransform(zlib.brotliDecompressSync),
      compressZstdSync: () =>
        __quenchZlibSyncIterTransform(zlib.zstdCompressSync),
      decompressZstdSync: () =>
        __quenchZlibSyncIterTransform(zlib.zstdDecompressSync),
    };
  }
  return __quenchOriginalRequireWithZlibIter(specifier);
};
"#;
