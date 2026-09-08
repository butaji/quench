// VM micro-case 007
// family=numeric; operation=float-rounding; variant=7; work_units=4680; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let value = 6.125; for (let i = 0; i < 4680; i++) value = Math.fround(value * 1.00031 - 0.00017); return Math.fround(value);
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "24.335765838623047", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
