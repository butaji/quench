// VM micro-case 050
// family=functions; operation=mutual-recursion; variant=10; work_units=940; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function even(value) { return value === 0 ? true : odd(value - 1); } function odd(value) { return value === 0 ? false : even(value - 1); } let total = 0; for (let i = 0; i < 940; i++) if (even(12 + (i & 3))) total++; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
