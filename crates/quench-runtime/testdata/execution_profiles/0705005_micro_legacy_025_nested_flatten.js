// VM micro-case 025
// family=arrays; operation=nested-flatten; variant=5; work_units=260; memory=retained-ring
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
const retained = [];
function microRun() {
  const nested = Array.from({ length: 260 }, (_, i) => [i, [i + 4, [i ^ 5]]]); const flat = nested.flat(2); retained.push(flat); if (retained.length > 8) retained.shift(); return 8000 + flat.length;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "8780", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
