// VM micro-case 049
// family=functions; operation=function-property-shapes; variant=9; work_units=660; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const functions = Array.from({ length: 660 }, (_, i) => { const fn = (x) => x + i; fn.tag = i & 3; return fn; }); let total = 0; for (const fn of functions) total += fn(fn.tag); return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "218460", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
