// VM micro-case 067
// family=iterables; operation=generator-throw; variant=7; work_units=234; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function* values() { for (let i = 0; i < 234; i++) { try { if ((i + 6) % 13 === 0) throw i; yield i; } catch (error) { yield error & 7; } } } let total = 0; for (const value of values()) total += value; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "25213", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
