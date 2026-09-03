// VM micro-case 066
// family=iterables; operation=iterator-close; variant=6; work_units=200; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let closed = 0; for (let pass = 0; pass < 200; pass++) { const iterable = { [Symbol.iterator]() { let i = 0; return { next() { return { value: i++, done: false }; }, return() { closed++; return { done: true }; } }; } }; for (const value of iterable) { if (value === 17) break; } } return closed;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
