const assert = require("assert");
const { toWritable } = require("stream/iter");

const writer = {
  write() {
    return Promise.resolve();
  },
  end() {
    return Promise.resolve();
  },
};
const writable = toWritable(writer);
assert.strictEqual(writable.writableHighWaterMark, Number.MAX_SAFE_INTEGER);
assert.strictEqual(writable._writev, null);
assert.throws(() => toWritable(null), { code: "ERR_INVALID_ARG_TYPE" });
