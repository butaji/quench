const zlib = require("zlib");

const cases = [
  ["gzip", "gunzip", "Gzip", "Gunzip"],
  ["deflate", "inflate", "Deflate", "Inflate"],
  ["deflateRaw", "inflateRaw", "DeflateRaw", "InflateRaw"],
  ["brotliCompress", "brotliDecompress", "BrotliCompress", "BrotliDecompress"],
];

let pending = cases.length * 2;
for (const [compress, decompress, compressClass, decompressClass] of cases) {
  zlib[compress]("info", { info: true }, (error, result) => {
    if (error) throw error;
    if (!(result.engine instanceof zlib[compressClass])) {
      throw new Error(`${compress} engine identity missing`);
    }
    zlib[decompress](result.buffer, { info: true }, (error2, result2) => {
      if (error2) throw error2;
      if (!(result2.engine instanceof zlib[decompressClass])) {
        throw new Error(`${decompress} engine identity missing`);
      }
      if (--pending === 0) console.log("zlib info engine matrix passed");
    });
  });
}
