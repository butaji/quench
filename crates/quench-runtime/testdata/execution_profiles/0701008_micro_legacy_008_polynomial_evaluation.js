// VM micro-case 008
// family=numeric; operation=polynomial-evaluation; variant=8; work_units=5140; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 5140; i++) { const x = (i + 7) * 0.01; total += (((x * 1.7 - 0.4) * x + 2.1) * x - 3.3); } return Math.round(total);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "296595029", "exact encoded result");
return __profileResult;
