// VM micro-case 058
// family=strings; operation=case-folding; variant=8; work_units=336; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = ("MiXeD-7-Straße-").repeat(3); let total = 0; for (let i = 0; i < 336; i++) total += text.toLowerCase().toUpperCase().length; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
