// VM micro-case 062
// family=iterables; operation=generator-yield; variant=2; work_units=260; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function* sequence() { for (let i = 0; i < 260; i++) yield (i * i + 1) & 255; } let total = 0; for (const value of sequence()) total += value; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
