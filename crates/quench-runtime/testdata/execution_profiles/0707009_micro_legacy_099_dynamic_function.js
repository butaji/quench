// VM micro-case 099
// family=meta; operation=dynamic-function; variant=9; work_units=126; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 126; i++) total += Function("x", "return (x * x + 8) | 0")(i); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "659883", "exact encoded result");
return __profileResult;
