const zlib = require("zlib");
const source = new DataView(new TextEncoder().encode("blah".repeat(8)).buffer);
const compressed = zlib.gzipSync(source, { level: 9, chunkSize: 1024 });
const result = zlib.gunzipSync(compressed, { level: 9, chunkSize: 1024 });
if (result.toString() !== "blah".repeat(8)) {
  throw new Error("DataView zlib roundtrip failed");
}
console.log("zlib DataView roundtrip passed");
