// VM micro-case 051
// family=strings; operation=concat-rope; variant=1; work_units=336; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let text = ""; for (let i = 0; i < 336; i++) text += String.fromCharCode(65 + ((i + 0) % 26)) + i; return text.length;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "1234", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
