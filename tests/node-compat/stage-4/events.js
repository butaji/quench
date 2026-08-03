const assert = require("assert");
let seen = false;
process.once("quench-event", (value) => {
  seen = value;
});
assert.strictEqual(process.emit("quench-event", true), true);
assert.strictEqual(seen, true);
let timerRan = false;
setTimeout(() => {
  timerRan = true;
}, 0);
queueMicrotask(() => {
  if (!timerRan) throw new Error("timer did not run");
});
