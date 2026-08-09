const assert = require("assert");
const { Writable } = require("stream");
const { fromWritable, ondrain } = require("stream/iter");

const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  }
});
const writer = fromWritable(writable);
assert.strictEqual(writer.writeSync(new Uint8Array(1)), false);
assert.strictEqual(writer.writevSync([new Uint8Array(1)]), false);
assert.strictEqual(writer.endSync(), -1);
assert.strictEqual(ondrain(writer) instanceof Promise, true);
