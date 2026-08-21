const assert = require("assert");
const stream = require("stream");
class TestWritable extends stream.Writable {
  _write(_chunk, _encoding, callback) {
    callback();
  }
  _final(callback) {
    process.nextTick(callback);
    process.nextTick(callback);
  }
}
const writable = new TestWritable();
console.log(
  "methods",
  typeof writable._write,
  typeof writable._final,
  Object.getPrototypeOf(writable) === TestWritable.prototype,
  typeof TestWritable.prototype._final,
  writable.constructor?.name,
);
assert.strictEqual(typeof writable._final, "function");
writable.on("error", () => console.log("error observed"));
writable.end("data");
setTimeout(() => console.log("writable prototype methods passed"), 20);
