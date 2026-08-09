const assert = require("assert");
const order = [];
const cancelled = setTimeout(() => order.push("cancelled"), 1);
clearTimeout(cancelled);
setTimeout(() => {
  order.push("timer");
  assert.deepStrictEqual(order, ["microtask", "timer"]);
}, 1);
queueMicrotask(() => {
  order.push("microtask");
});
