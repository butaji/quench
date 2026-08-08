const assert = require("assert");
const { Readable } = require("stream");

let reached = false;
let resolve;
const pending = new Promise((res) => (resolve = res));
const stream = Readable.from(
  (async function* () {
    yield 1;
    await pending;
    reached = true;
    yield 2;
  })()
);

stream
  .take(1)
  .toArray()
  .then((values) => {
    assert.deepStrictEqual(values, [1]);
    assert.strictEqual(reached, false);
    resolve();
    console.log("take cleanup passed");
  });
