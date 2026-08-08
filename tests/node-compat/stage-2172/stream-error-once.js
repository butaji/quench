const assert = require("assert");
const { Readable, Writable } = require("stream");

const writable = new Writable();
writable.on("error", () => {});
writable.end();
writable.write("h");
writable.write("h");

let errors = 0;
const readable = new Readable();
readable.on("error", (error) => {
  errors++;
  assert.strictEqual(error.code, "ERR_STREAM_PUSH_AFTER_EOF");
});
readable.push(null);
readable.push("h");
readable.push("h");
setTimeout(() => {
  assert.strictEqual(errors, 1);
  console.log("stream error once pass");
}, 0);
