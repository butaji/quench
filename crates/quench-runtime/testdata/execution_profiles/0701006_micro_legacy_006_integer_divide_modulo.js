// VM micro-case 006
// family=numeric; operation=integer-divide-modulo; variant=6; work_units=4220; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 1; i <= 4220; i++) total = (total + (i * 7919) % (102)) | 0; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "213048", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
