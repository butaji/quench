const assert = require("assert");

const events = [];
const timeout = setTimeout(() => events.push("cleared timeout"), 0);
const interval = setInterval(() => events.push("cleared interval"), 1);
clearTimeout(timeout);
clearInterval(interval);
assert.strictEqual(timeout.refresh(), timeout);
assert.strictEqual(interval.refresh(), interval);

queueMicrotask(() => events.push("microtask"));
setTimeout(() => {
  events.push("timer");
  assert.deepStrictEqual(events, ["microtask", "timer"]);
}, 1);
