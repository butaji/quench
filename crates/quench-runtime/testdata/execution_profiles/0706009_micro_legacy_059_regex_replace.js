// VM micro-case 059
// family=strings; operation=regex-replace; variant=9; work_units=1440; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = ("item-8-value-").repeat(60); const pattern = /item-(\d+)-/g; let total = 0; for (let i = 0; i < 24; i++) total += text.replace(pattern, "x$1").length; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "11520", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
