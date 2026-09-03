// VM micro-case 076
// family=collections; operation=map-delete-reinsert; variant=6; work_units=440; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const map = new Map(); for (let i = 0; i < 440; i++) map.set(i, i ^ 5); let total = 0; for (let i = 0; i < 440; i++) { total += map.get(i); map.delete(i); map.set(i, total); } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
