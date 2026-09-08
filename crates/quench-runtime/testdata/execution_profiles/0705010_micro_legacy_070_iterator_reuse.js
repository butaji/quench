// VM micro-case 070
// family=iterables; operation=iterator-reuse; variant=10; work_units=1260; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = Array.from({ length: 21 }, (_, i) => i + 9); let total = 0; for (let pass = 0; pass < 60; pass++) for (const value of values) total ^= value; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "0", "exact encoded result");
return __profileResult;
