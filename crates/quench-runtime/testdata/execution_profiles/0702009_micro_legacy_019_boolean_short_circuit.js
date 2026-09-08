// VM micro-case 019
// family=control; operation=boolean-short-circuit; variant=9; work_units=8640; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 8640; i++) { const a = (i & 1) === 0; const b = i % 3 === 2; if ((a && b) || (!a && !b)) total++; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "4320", "exact encoded result");
return __profileResult;
