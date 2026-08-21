const assert = require("assert");

let polls = 0;
globalThis.__quench_io_poll = () => {
  polls++;
};

setImmediate(() => {
  assert.ok(polls >= 1);
  console.log("host I/O poll hook passed");
});
