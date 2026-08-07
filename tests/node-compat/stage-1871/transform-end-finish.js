const assert = require("assert");
const { Transform } = require("stream");

const stream = new Transform({
  transform(chunk, encoding, callback) {
    callback();
  },
});
let ended = false;
let finished = false;
stream.on("end", () => (ended = true));
stream.on("finish", () => (finished = true));
stream.end();
stream.resume();

setImmediate(() => {
  assert.strictEqual(finished, true);
  assert.strictEqual(ended, true);
});
