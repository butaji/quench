const assert = require("assert");

Buffer.allocUnsafe(10);
Buffer.from("deadbeaf", "hex");
for (
  const view of [
    new Uint16Array([0xffff]),
    new Uint16Array([0, 0xffff]),
    new Uint32Array([0xffffffff]),
  ]
) {
  Buffer.copyBytesFrom(view);
}

const source = new Float64Array([1, 2, 3, 4]);
const result = Buffer.copyBytesFrom(source, 1, 2);
assert.strictEqual(new Float64Array(result.buffer, result.byteOffset, 2)[0], 2);
assert.strictEqual(new Float64Array(result.buffer, result.byteOffset, 2)[1], 3);
console.log("buffer copyBytesFrom alignment passed");
