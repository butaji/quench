const assert = require("assert");
const { Writable } = require("stream");
const writable = new Writable({ highWaterMark: 2 });
let drained = false;
writable.on("drain", () => {
  drained = true;
});
assert.strictEqual(
  writable.write("abc", () => {}),
  false,
);
queueMicrotask(() => assert.strictEqual(drained, true));
