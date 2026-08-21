const assert = require("assert");
const { Readable } = require("stream");

const readable = new Readable({
  autoDestroy: false,
  read() {
    this.push(null);
    this.push("late");
  },
});

readable.on("error", (error) => {
  assert.strictEqual(error.code, "ERR_STREAM_PUSH_AFTER_EOF");
  assert.strictEqual(readable.errored, error);
});
readable.resume();

console.log("stream push after eof read pass");
