// VM micro-case 079
// family=collections; operation=map-object-values; variant=9; work_units=560; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const map = new Map(); for (let i = 0; i < 560; i++) map.set(i, { value: i + 8, parity: i & 1 }); let total = 0; for (const object of map.values()) total += object.value + object.parity; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "161280", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
