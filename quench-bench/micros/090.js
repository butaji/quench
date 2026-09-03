// VM micro-case 090
// family=typed-memory; operation=typed-sort-copy; variant=10; work_units=1290; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new Int32Array(1290); for (let i = 0; i < values.length; i++) values[i] = (1290 - i) ^ 9; const copy = Array.from(values).sort((a, b) => a - b); return copy[0] + copy.at(-1);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
