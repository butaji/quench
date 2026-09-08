// VM micro-case 083
// family=typed-memory; operation=float64-transform; variant=3; work_units=660; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new Float64Array(660); let total = 0; for (let i = 0; i < values.length; i++) { values[i] = Math.sin(i + 2) * 0.5; total += values[i]; } return Math.round(total * 1e6);
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "137798", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
