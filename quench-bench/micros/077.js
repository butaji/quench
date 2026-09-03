// VM micro-case 077
// family=collections; operation=set-churn; variant=7; work_units=480; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const set = new Set(); let total = 0; for (let i = 0; i < 480; i++) { set.add((i + 6) % 31); if (set.has(i % 31)) total++; if (i & 1) set.delete((i + 3) % 31); } return total + set.size;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
