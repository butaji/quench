const assert = require("assert");
const { Buffer } = require("buffer");

for (
  const allocate of [
    () => Buffer.alloc(2, 1),
    () => Buffer.allocUnsafe(2),
    () => Buffer.allocUnsafeSlow(2),
  ]
) {
  const buffer = allocate();
  assert.strictEqual(typeof buffer.copy, "function");
  assert.strictEqual(typeof buffer.compare, "function");
  assert.strictEqual(typeof buffer.fill, "function");
}

const source = Buffer.from([1, 2]);
const target = Buffer.alloc(2);
assert.strictEqual(source.copy(target), 2);
assert.deepStrictEqual(Array.from(target), [1, 2]);
assert.strictEqual(source.compare(target), 0);
