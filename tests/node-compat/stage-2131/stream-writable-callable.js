const assert = require("assert");
const { Writable } = require("stream");

function LegacyWritable() {
  assert.strictEqual(this.destroyed, false);
  Writable.call(this, {
    write(_chunk, _encoding, callback) {
      callback();
    }
  });
}
Object.setPrototypeOf(LegacyWritable.prototype, Writable.prototype);
Object.setPrototypeOf(LegacyWritable, Writable);

const stream = new LegacyWritable();
assert.ok(stream instanceof Writable);
assert.strictEqual(stream.destroyed, false);

console.log("stream writable callable pass");
