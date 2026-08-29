// VM micro-case 198
// family=objects; level=4; depth=48
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const queue = Array.from({ length: 12 }, (_, id) => ({ id, budget: id % 4 + 1 })); let ticks = 0;
while (queue.length) { const task = queue.shift(); ticks += task.budget; if (task.budget > 1) queue.push({ id: task.id, budget: task.budget - 1 }); }
assert(ticks > 0 && queue.length === 0, "scheduler queue");
return ticks;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
