// VM micro-case 039
// family=objects; operation=sealed-frozen-properties; variant=9; work_units=1080; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 1080; i++) { const object = Object.freeze({ a: i + 8, b: i ^ 8 }); total += object.a + object.b; } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "1174024", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
