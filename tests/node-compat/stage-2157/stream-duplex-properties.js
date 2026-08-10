const assert = require("assert");
const { Duplex } = require("stream");

const first = Duplex({ objectMode: true, highWaterMark: 100 });
assert.strictEqual(first.readableObjectMode, true);
assert.strictEqual(first.writableObjectMode, true);
assert.strictEqual(first.readableHighWaterMark, 100);
assert.strictEqual(first.writableHighWaterMark, 100);

const second = Duplex({
  readableObjectMode: false,
  readableHighWaterMark: 10,
  writableObjectMode: true,
  writableHighWaterMark: 100,
});
assert.strictEqual(second.readableObjectMode, false);
assert.strictEqual(second.writableObjectMode, true);
assert.strictEqual(second.readableHighWaterMark, 10);
assert.strictEqual(second.writableHighWaterMark, 100);

console.log("stream duplex properties pass");
