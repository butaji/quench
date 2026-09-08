// VM micro-case 035
// family=objects; operation=symbol-key-retention; variant=5; work_units=200; memory=retained-ring
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
const retained = [];
function microRun() {
  let total = 0; for (let i = 0; i < 200; i++) { const key = Symbol("field-" + (i & 7)); const object = { [key]: i, visible: i + 5 }; retained.push(object); if (retained.length > 16) retained.shift(); total += object[key] + object.visible; } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "40800", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
