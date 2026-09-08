// VM micro-case 069
// family=iterables; operation=destructure-defaults; variant=9; work_units=260; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 260; i++) { const [a = 1, b = 2, c = 3] = (i & 1) ? [i] : []; total += a + b + c; } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "18330", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
