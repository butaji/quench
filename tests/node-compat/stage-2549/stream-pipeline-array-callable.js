const assert = require("assert");
const { PassThrough, Readable, Writable, pipeline } = require("stream");

const source = Readable.from(["a"]);
const sink = Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const pass = PassThrough();
assert.ok(pass instanceof PassThrough);

pipeline([source, pass, sink], (error) => {
  assert.ifError(error);
  console.log("stream pipeline array/callable passed");
});
