// VM micro-case 061
// family=iterables; operation=custom-iterator; variant=1; work_units=240; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const iterable = { [Symbol.iterator]() { let index = 0; return { next() { return index < 240 ? { value: index++ * 1, done: false } : { done: true }; } }; } }; let total = 0; for (const value of iterable) total += value; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "28680", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
