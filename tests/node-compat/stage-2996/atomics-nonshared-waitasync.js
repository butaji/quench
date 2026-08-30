"use strict";

const assert = require("assert");

(async () => {
  const ordinary = new Int32Array(new ArrayBuffer(4));
  assert.strictEqual(Atomics.add(ordinary, 0, 1), 0);
  assert.strictEqual(ordinary[0], 1);
  assert.strictEqual(Atomics.notify(ordinary, 0), 0);

  const shared = new Int32Array(new SharedArrayBuffer(4));
  const pending = Atomics.waitAsync(shared, 0, 0, 10);
  assert.strictEqual(pending.async, true);
  assert.strictEqual(await pending.value, "timed-out");
  console.log("ok");
})();
