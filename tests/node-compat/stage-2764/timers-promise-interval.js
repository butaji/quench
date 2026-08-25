"use strict";
const assert = require("assert");
const { setInterval } = require("timers/promises");
(async () => {
  const iterator = setInterval(0, "tick");
  assert.strictEqual(typeof iterator.next, "function");
  assert.strictEqual(typeof iterator.return, "function");
  const first = await iterator.next();
  assert.deepStrictEqual(first, { value: "tick", done: false });
  const done = await iterator.return();
  assert.deepStrictEqual(done, { value: undefined, done: true });
})();
