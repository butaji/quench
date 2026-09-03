// VM micro-case 072
// family=collections; operation=set-membership; variant=2; work_units=560; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const set = new Set(Array.from({ length: 280 }, (_, i) => i * 2)); let found = 0; for (let i = 0; i < 560; i++) if (set.has(i)) found++; return found + set.size;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
