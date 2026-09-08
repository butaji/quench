// VM micro-case 001
// family=numeric; operation=integer-recurrence; variant=1; work_units=1920; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let value = 3; for (let i = 0; i < 1920; i++) value = (value * 1664525 + i + 11) | 0; return value >>> 0;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "1563355587", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
