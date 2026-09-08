// VM micro-case 065
// family=iterables; operation=forof-set; variant=5; work_units=384; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new Set(); for (let i = 0; i < 384; i++) values.add((i * 7) % 229); let total = 0; for (const value of values) total += value; return total + values.size;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "26335", "exact encoded result");
return __profileResult;
