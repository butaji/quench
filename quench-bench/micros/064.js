// VM micro-case 064
// family=iterables; operation=spread-merge; variant=4; work_units=225; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const left = Array.from({ length: 225 }, (_, i) => i); const right = Array.from({ length: 225 }, (_, i) => i + 3); const merged = [...left, ...right, 3]; return merged.filter((value) => (value & 1) === 0).length;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
