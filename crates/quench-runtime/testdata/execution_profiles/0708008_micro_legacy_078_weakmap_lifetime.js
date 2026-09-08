// VM micro-case 078
// family=collections; operation=weakmap-lifetime; variant=8; work_units=520; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const weak = new WeakMap(); let total = 0; for (let i = 0; i < 520; i++) { const key = { index: i }; weak.set(key, i + 7); total += weak.get(key); } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "138580", "exact encoded result");
return __profileResult;
