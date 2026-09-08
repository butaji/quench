// VM micro-case 057
// family=strings; operation=template-render; variant=7; work_units=624; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 624; i++) { const rendered = `row-6-${i}-${i * i}`; total += rendered.length; } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "9411", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
