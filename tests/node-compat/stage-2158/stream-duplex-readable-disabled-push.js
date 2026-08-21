const assert = require("assert");
const { Duplex } = require("stream");

const duplex = Duplex({ readable: false });
assert.strictEqual(duplex.readable, false);
duplex.push("late");
duplex.on("error", (error) => {
  assert.strictEqual(error.code, "ERR_STREAM_PUSH_AFTER_EOF");
  console.log("stream duplex disabled readable push pass");
});
