const assert = require("assert");
const { CompressionStream, DecompressionStream } = require("stream/web");

for (const format of ["gzip", "deflate", "deflate-raw", "brotli"]) {
  assert.strictEqual(
    new CompressionStream(format)[Symbol.toStringTag],
    "CompressionStream",
  );
  assert.strictEqual(
    new DecompressionStream(format)[Symbol.toStringTag],
    "DecompressionStream",
  );
}
for (const format of [1, "hello", false, {}]) {
  assert.throws(() => new CompressionStream(format), {
    code: "ERR_INVALID_ARG_VALUE",
  });
  assert.throws(() => new DecompressionStream(format), {
    code: "ERR_INVALID_ARG_VALUE",
  });
}
console.log("compression streams passed");
