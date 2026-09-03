// VM micro-case 017
// family=control; operation=labeled-nesting; variant=7; work_units=10000; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; outer: for (let row = 0; row < 400; row++) for (let col = 0; col < 25; col++) { if ((row + col + 6) % 11 === 0) continue outer; total += row ^ col; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
