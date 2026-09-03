// VM micro-case 020
// family=control; operation=recurrence-branch; variant=10; work_units=4660; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let a = 10; let b = 11; for (let i = 0; i < 4660; i++) { const next = (a + b) & 0xffff; a = b; b = next ^ (i & 31); } return b;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
