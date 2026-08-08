const assert = require("assert");
const { Duplex } = require("stream");

const error = "async function failure";
Duplex.from(async () => Promise.reject(error)).on("error", (received) => {
  assert.strictEqual(received, error);
  console.log("stream duplex from function pass");
});

assert.throws(() => Duplex.from(() => {}), {
  code: "ERR_INVALID_RETURN_VALUE"
});
