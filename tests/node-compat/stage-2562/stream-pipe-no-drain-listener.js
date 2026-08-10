const assert = require("assert");
const { PassThrough, Readable } = require("stream");

const source = new Readable({ read() {} });
const pass = source.pipe(
  new PassThrough({ objectMode: true, highWaterMark: 2 }),
);
let finished = false;
pass.on("finish", () => finished = true);
assert.strictEqual(pass.listenerCount("drain"), 0);
source.push("asd");
assert.strictEqual(pass.listenerCount("drain"), 0);
process.nextTick(() => {
  source.push("asd");
  assert.strictEqual(pass.listenerCount("drain"), 0);
  source.push(null);
  assert.strictEqual(pass.listenerCount("drain"), 0);
  setImmediate(() => {
    assert.strictEqual(finished, false);
    console.log("stream pipe no drain listener passed");
  });
});
