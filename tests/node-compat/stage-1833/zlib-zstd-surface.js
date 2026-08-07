const zlib = require("zlib");

for (
  const name of [
    "zstdCompress",
    "zstdDecompress",
    "zstdCompressSync",
    "zstdDecompressSync",
    "ZstdCompress",
    "ZstdDecompress",
  ]
) {
  if (typeof zlib[name] !== "function") {
    throw new Error(`missing zlib.${name}`);
  }
}

console.log("zlib zstd surface passed");
