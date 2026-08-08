const assert = require("assert");
const {
  ByteLengthQueuingStrategy,
  CountQueuingStrategy
} = require("stream/web");

const bytes = new ByteLengthQueuingStrategy({ highWaterMark: 8 });
assert.strictEqual(bytes.highWaterMark, 8);
assert.strictEqual(bytes.size(new Uint8Array(3)), 3);
const count = new CountQueuingStrategy({ highWaterMark: 2 });
assert.strictEqual(count.highWaterMark, 2);
assert.strictEqual(count.size("anything"), 1);
assert.throws(
  () => Reflect.get(ByteLengthQueuingStrategy.prototype, "size", {}),
  /private member/
);
console.log("queuing strategies passed");
