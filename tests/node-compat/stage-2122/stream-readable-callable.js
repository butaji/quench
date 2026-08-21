const assert = require("assert");
const { Readable } = require("stream");

function LegacyReadable() {
  assert.strictEqual(this.destroyed, false);
  Readable.call(this, { read() {} });
}
Object.setPrototypeOf(LegacyReadable.prototype, Readable.prototype);
Object.setPrototypeOf(LegacyReadable, Readable);

const stream = new LegacyReadable();
assert.ok(stream instanceof Readable);
assert.strictEqual(stream.destroyed, false);
assert.strictEqual(typeof Readable.from, "function");

console.log("stream readable callable pass");
