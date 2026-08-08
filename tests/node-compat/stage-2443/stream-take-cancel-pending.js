const assert = require("assert");
const { Readable } = require("stream");

let reached = false;
let release;
const pending = new Promise((resolve) => {
  release = resolve;
});
const source = Readable.from(
  (async function* () {
    yield 1;
    await pending;
    reached = true;
    yield 2;
  })()
);

source
  .take(1)
  .toArray()
  .then((values) => {
    assert.deepStrictEqual(values, [1]);
    assert.strictEqual(reached, false);
    release();
    console.log("stream take cancellation passed");
  });
