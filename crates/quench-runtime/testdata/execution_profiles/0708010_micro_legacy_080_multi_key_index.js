// VM micro-case 080
// family=collections; operation=multi-key-index; variant=10; work_units=600; memory=retained-ring
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
const index = new Map();
function microRun() {
  for (let i = 0; i < 600; i++) { const key = (i % 7) + ":" + (i % 11); index.set(key, i); } let total = 0; for (const key of index.keys()) total += index.get(key); return total + index.size;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "43274", "exact encoded result");
return __profileResult;
