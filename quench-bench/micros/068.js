// VM micro-case 068
// family=iterables; operation=iterator-composition; variant=8; work_units=228; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function* source() { for (let i = 0; i < 228; i++) yield i + 7; } function* mapped(input) { for (const value of input) yield value * 3; } let total = 0; for (const value of mapped(source())) total += value; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
