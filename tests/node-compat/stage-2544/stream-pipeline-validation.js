const assert = require("assert");
const { Readable, Writable, pipeline } = require("stream");

const readable = new Readable({ read() {} });
const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});

assert.throws(() => pipeline(readable, () => {}), {
  code: "ERR_MISSING_ARGS",
});
assert.throws(() => pipeline(readable, writable), {
  code: "ERR_MISSING_ARGS",
});
assert.throws(() => pipeline(readable, writable, writable), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => pipeline(), {
  code: "ERR_INVALID_ARG_TYPE",
});

console.log("stream pipeline validation passed");
