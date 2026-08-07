const assert = require("assert");
const { Duplex } = require("stream");

const stream = new Duplex({
  read() {},
  write(_chunk, _encoding, callback) {
    callback();
  },
});
assert.strictEqual(stream.push(null), true);
assert.strictEqual(stream.setEncoding("utf8"), stream);
