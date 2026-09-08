// VM micro-case 009
// family=numeric; operation=numeric-conversion; variant=9; work_units=2800; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 2800; i++) total += Number.parseInt(String((i * 27) % 100000), 10); return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "105802200", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
