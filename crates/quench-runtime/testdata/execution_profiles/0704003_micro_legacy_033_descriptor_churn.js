// VM micro-case 033
// family=objects; operation=descriptor-churn; variant=3; work_units=420; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const object = {}; let total = 0; for (let i = 0; i < 420; i++) { Object.defineProperty(object, "p" + (i % 5), { value: i + 2, writable: true, configurable: true, enumerable: (i & 1) === 0 }); total += Object.getOwnPropertyDescriptor(object, "p" + (i % 5)).value; } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "88830", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
