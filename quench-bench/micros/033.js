// VM micro-case 033
// family=objects; operation=descriptor-churn; variant=3; work_units=420; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const object = {}; let total = 0; for (let i = 0; i < 420; i++) { Object.defineProperty(object, "p" + (i % 5), { value: i + 2, writable: true, configurable: true, enumerable: (i & 1) === 0 }); total += Object.getOwnPropertyDescriptor(object, "p" + (i % 5)).value; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
