const assert = require("assert");
const zlib = require("zlib");

for (
  const [compress, expected] of [
    [zlib.gzipSync, "gzip"],
    [zlib.deflateSync, "deflate"],
  ]
) {
  const restored = [];
  zlib
    .createUnzip()
    .on("data", (chunk) => restored.push(chunk))
    .on(
      "end",
      () => assert.strictEqual(Buffer.concat(restored).toString(), expected),
    )
    .end(compress(expected));
}
