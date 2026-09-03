// VM micro-case 061
// family=iterables; operation=custom-iterator; variant=1; work_units=240; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const iterable = { [Symbol.iterator]() { let index = 0; return { next() { return index < 240 ? { value: index++ * 1, done: false } : { done: true }; } }; } }; let total = 0; for (const value of iterable) total += value; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
