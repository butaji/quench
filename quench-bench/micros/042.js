// VM micro-case 042
// family=functions; operation=recursive-fold; variant=2; work_units=460; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function fold(value, depth) { return depth === 0 ? value : fold((value * 3 + depth) | 0, depth - 1); } let total = 0; for (let i = 0; i < 460; i++) total ^= fold(i + 1, 6); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
