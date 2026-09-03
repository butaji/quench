// VM micro-case 067
// family=iterables; operation=generator-throw; variant=7; work_units=234; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function* values() { for (let i = 0; i < 234; i++) { try { if ((i + 6) % 13 === 0) throw i; yield i; } catch (error) { yield error & 7; } } } let total = 0; for (const value of values()) total += value; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
