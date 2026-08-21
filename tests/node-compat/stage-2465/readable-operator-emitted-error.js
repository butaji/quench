const assert = require("assert");
const { Readable } = require("stream");

const failure = new Error("boom");
let stream;
stream = Readable.from([1, 2, 3, 4, 5])
  .map(async (value) => {
    if (value === 3) stream.emit("error", failure);
    return value * 2;
  })
  .map((value) => value * 2);

let completed = false;
assert.rejects(stream.toArray(), failure).then(() => {
  completed = true;
});

process.on("beforeExit", () => {
  assert.strictEqual(completed, true);
});
