// VM micro-case 036
// family=objects; operation=property-order-churn; variant=6; work_units=570; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 570; i++) { const object = {}; object["z" + i] = i; object.a = i + 1; object["m" + (i & 3)] = i + 2; total += Reflect.ownKeys(object).length; } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "1710", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
