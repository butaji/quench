// VM micro-case 058
// family=strings; operation=case-folding; variant=8; work_units=336; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = ("MiXeD-7-Straße-").repeat(3); let total = 0; for (let i = 0; i < 336; i++) total += text.toLowerCase().toUpperCase().length; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "16128", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
