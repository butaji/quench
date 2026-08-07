const assert = require("assert");
const { Writable } = require("stream");

const writable = Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});

assert(writable instanceof Writable);
