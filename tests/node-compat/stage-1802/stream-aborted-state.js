const assert = require("assert");
const { Readable, Writable } = require("stream");

const readable = new Readable({ read() {} });
assert.strictEqual(readable.readableAborted, false);
readable.destroy();
assert.strictEqual(readable.readableAborted, true);

const completedReadable = new Readable({ read() {} });
completedReadable.push(null);
completedReadable.on("end", () => {
  completedReadable.destroy();
  assert.strictEqual(completedReadable.readableAborted, false);
});
completedReadable.resume();

const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
assert.strictEqual(writable.writableAborted, false);
writable.destroy();
assert.strictEqual(writable.writableAborted, true);

const duplex = new (require("stream").Duplex)({ readable: false });
assert.strictEqual(duplex.readable, false);
assert.strictEqual(duplex.readableAborted, false);

console.log("stream aborted state passed");
