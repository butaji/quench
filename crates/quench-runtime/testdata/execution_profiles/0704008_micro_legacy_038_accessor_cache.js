// VM micro-case 038
// family=objects; operation=accessor-cache; variant=8; work_units=1340; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let value = 7; const object = { get field() { return value; }, set field(next) { value = next ^ 3; } }; let total = 0; for (let i = 0; i < 1340; i++) { object.field = i; total += object.field; } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "897130", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
