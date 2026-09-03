// VM micro-case 010
// family=numeric; operation=typed-lane-arithmetic; variant=10; work_units=4545; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new Int32Array(4545); let total = 0; for (let i = 0; i < values.length; i++) { values[i] = (i ^ 279) - 9; total += values[i] * values[i]; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
