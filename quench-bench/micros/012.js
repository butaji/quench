// VM micro-case 012
// family=control; operation=nested-count; variant=2; work_units=9360; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let row = 0; row < 40; row++) for (let col = 0; col < 26; col++) for (let lane = 0; lane < 9; lane++) total += (row ^ col ^ lane) & 3; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
