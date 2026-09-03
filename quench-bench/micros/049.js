// VM micro-case 049
// family=functions; operation=function-property-shapes; variant=9; work_units=660; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const functions = Array.from({ length: 660 }, (_, i) => { const fn = (x) => x + i; fn.tag = i & 3; return fn; }); let total = 0; for (const fn of functions) total += fn(fn.tag); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
