// VM micro-case 075
// family=collections; operation=bounded-map-cache; variant=5; work_units=400; memory=retained-ring
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
const cache = new Map();
function microRun() {
  for (let i = 0; i < 400; i++) { cache.set((i + 4) % 17, i * i); if (cache.size > 12) cache.delete(cache.keys().next().value); } let total = 0; for (const value of cache.values()) total += value; return total + 12000;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "1870250", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
