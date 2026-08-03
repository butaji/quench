const assert = require("assert");
const zlib = require("zlib");

const input = Buffer.from("unzip detects both formats");
const deflated = zlib.deflateSync(input);
const gzipped = zlib.gzipSync(input);
assert.strictEqual(zlib.unzipSync(deflated).toString(), input.toString());
assert.strictEqual(zlib.unzipSync(gzipped).toString(), input.toString());

zlib.unzip(gzipped, (error, result) => {
  assert.ifError(error);
  assert.strictEqual(result.toString(), input.toString());
  console.log("zlib unzip passed");
});
