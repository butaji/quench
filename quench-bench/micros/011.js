// VM micro-case 011
// family=control; operation=branch-predict; variant=1; work_units=6400; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let score = 0; let state = 1; for (let i = 0; i < 6400; i++) { state = (state * 1103515245 + 12345) >>> 0; if ((state & 3) === 0) score += i; else score -= i & 7; } return score;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
