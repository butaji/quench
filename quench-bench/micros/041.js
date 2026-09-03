// VM micro-case 041
// family=functions; operation=closure-capture; variant=1; work_units=1600; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function make(seed) { let value = seed; return () => value = (value * 31 + 7) | 0; } const next = make(1); let total = 0; for (let i = 0; i < 1600; i++) total ^= next(); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
