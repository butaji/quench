// VM micro-case 065
// family=iterables; operation=forof-set; variant=5; work_units=384; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new Set(); for (let i = 0; i < 384; i++) values.add((i * 7) % 229); let total = 0; for (const value of values) total += value; return total + values.size;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
