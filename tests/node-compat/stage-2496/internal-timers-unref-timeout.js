const assert = require("assert");
const { setUnrefTimeout } = require("internal/timers");

assert.throws(() => setUnrefTimeout(null), {
  code: "ERR_INVALID_ARG_TYPE"
});

let calls = 0;
const keepAlive = setTimeout(() => {}, 10);
const timer = setUnrefTimeout(
  (value) => {
    calls++;
    assert.strictEqual(value, "value");
    clearTimeout(keepAlive);
  },
  0,
  "value"
);
assert.strictEqual(timer.hasRef(), false);
assert.strictEqual(timer.refresh(), timer);
setImmediate(() => assert.strictEqual(calls, 1));

const order = [];
const refreshed = setTimeout(() => order.push("refreshed"), 1);
setTimeout(() => {
  assert.deepStrictEqual(order, []);
  order.push("peer");
}, 1);
refreshed.refresh();
setTimeout(() => {
  assert.deepStrictEqual(order, ["peer", "refreshed"]);
}, 10);
