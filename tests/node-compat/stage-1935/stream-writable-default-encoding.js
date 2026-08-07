const assert = require("assert");
const { Writable } = require("stream");

const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
assert.strictEqual(writable.setDefaultEncoding("base64"), writable);
assert.strictEqual(writable.writableDefaultEncoding, "base64");
assert.throws(() => writable.setDefaultEncoding("not-an-encoding"), {
  code: "ERR_UNKNOWN_ENCODING",
});
console.log("stream writable default encoding passed");
