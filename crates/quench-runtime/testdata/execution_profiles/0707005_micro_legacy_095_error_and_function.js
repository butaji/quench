// VM micro-case 095
// family=meta; operation=error-and-function; variant=5; work_units=90; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 90; i++) { try { throw new TypeError("micro-" + (i + 4)); } catch (error) { total += error.message.length; } total += Function("x", "return x + 1")(4); } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "1164", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
