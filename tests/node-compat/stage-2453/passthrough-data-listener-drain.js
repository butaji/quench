const assert = require("assert");
const { PassThrough } = require("stream");

const source = new PassThrough();
const target = new PassThrough();
const chunk = Buffer.allocUnsafe(1000);
let bufferedWrites = 0;
let drained = false;
let consumed = 0;

while (target.write(chunk)) bufferedWrites++;

source.pipe(target);
target.on("drain", () => {
  drained = true;
  assert.strictEqual(source.isPaused(), false);
});
target.on("data", (data) => {
  consumed += data.length;
});

process.on("beforeExit", () => {
  const actual = { bufferedWrites, consumed, drained };
  assert.strictEqual(bufferedWrites > 0, true);
  assert.strictEqual(consumed > 0, true);
  assert.strictEqual(drained, true, JSON.stringify(actual));
});
