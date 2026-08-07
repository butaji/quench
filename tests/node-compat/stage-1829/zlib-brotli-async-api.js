const assert = require("assert");
const zlib = require("node:zlib");

assert.strictEqual(typeof zlib.brotliCompress, "function");
assert.strictEqual(typeof zlib.brotliDecompress, "function");
assert.strictEqual(typeof zlib.createBrotliCompress, "function");
assert.strictEqual(typeof zlib.createBrotliDecompress, "function");

const input = Buffer.from("brotli async api");
zlib.brotliCompress(input, (compressError, compressed) => {
  assert.ifError(compressError);
  zlib.brotliDecompress(compressed, (decompressError, restored) => {
    assert.ifError(decompressError);
    assert.deepStrictEqual(restored, input);

    const compressedStream = zlib.createBrotliCompress();
    const decompressedStream = zlib.createBrotliDecompress();
    const chunks = [];
    decompressedStream.on("data", (chunk) => chunks.push(chunk));
    decompressedStream.on("end", () => {
      assert.deepStrictEqual(Buffer.concat(chunks), input);
      console.log("zlib Brotli async API passed");
    });
    compressedStream.pipe(decompressedStream);
    compressedStream.end(input);
  });
});
