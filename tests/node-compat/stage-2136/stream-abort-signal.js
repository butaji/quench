const assert = require("assert");
const { Readable, addAbortSignal } = require("stream");

const controller = new AbortController();
const readable = addAbortSignal(
  controller.signal,
  new Readable({
    read() {
      this.push("data");
    },
  }),
);
readable.on("error", (error) => {
  assert.strictEqual(error.name, "AbortError");
  assert.strictEqual(error.code, "ABORT_ERR");
});
controller.abort();

console.log("stream abort signal pass");
