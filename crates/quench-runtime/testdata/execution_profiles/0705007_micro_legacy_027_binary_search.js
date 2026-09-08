// VM micro-case 027
// family=arrays; operation=binary-search; variant=7; work_units=5760; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = Array.from({ length: 600 }, (_, i) => i * 2); let hits = 0; for (let needle = 6; needle < 1200; needle += 3) { let lo = 0; let hi = values.length - 1; while (lo <= hi) { const mid = (lo + hi) >> 1; if (values[mid] === needle) { hits++; break; } if (values[mid] < needle) lo = mid + 1; else hi = mid - 1; } } return hits;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "199", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
