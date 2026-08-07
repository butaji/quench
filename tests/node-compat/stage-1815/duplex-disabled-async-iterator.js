const assert = require("assert");
const { Duplex } = require("stream");

const duplex = new Duplex({ readable: false });
assert.strictEqual(duplex.readable, false);
let count = 0;
(async () => {
  for await (const chunk of duplex) {
    count++;
    assert.fail(chunk);
  }
})().then(() => {
  assert.strictEqual(count, 0);
  console.log("disabled Duplex async iterator passed");
});
