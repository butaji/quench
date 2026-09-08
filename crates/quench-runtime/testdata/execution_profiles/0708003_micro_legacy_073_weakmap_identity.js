// VM micro-case 073
// family=collections; operation=weakmap-identity; variant=3; work_units=320; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const keys = Array.from({ length: 320 }, (_, i) => ({ id: i })); const weak = new WeakMap(keys.map((key) => [key, key.id + 2])); let total = 0; for (const key of keys) total += weak.get(key); return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "51680", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
