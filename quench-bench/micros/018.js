// VM micro-case 018
// family=control; operation=try-finally-control; variant=8; work_units=3980; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 3980; i++) { try { total += i; if ((i + 7) % 17 === 0) throw i; } catch (error) { total ^= error; } finally { total = (total + 1) | 0; } } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
