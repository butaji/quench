const assert = require("assert");
const zlib = require("zlib");

assert.strictEqual(typeof zlib.createZstdCompress, "function");
assert.strictEqual(typeof zlib.createZstdDecompress, "function");
assert.strictEqual(zlib.constants.ZSTD_e_end, 2);
for (const [factory, valid] of [
  [zlib.createGzip, [0, 4, 5]],
  [zlib.createBrotliCompress, [0, 1, 2, 3]],
  [zlib.createZstdCompress, [0, 1, 2]]
]) {
  for (const kind of valid) factory().flush(kind);
  for (const kind of [-1, 6, 100]) {
    assert.throws(() => factory().flush(kind), { code: "ERR_OUT_OF_RANGE" });
  }
  assert.doesNotThrow(() => factory().flush(NaN));
  assert.throws(() => factory().flush("invalid"), {
    code: "ERR_INVALID_ARG_TYPE"
  });
}
console.log("zlib flush surface passed");
