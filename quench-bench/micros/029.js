// VM micro-case 029
// family=arrays; operation=array-copy; variant=9; work_units=50400; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const source = Array.from({ length: 560 }, (_, i) => i + 8); let total = 0; for (let i = 0; i < 90; i++) { const copy = source.slice(i, source.length - i); total += copy.length + copy[0]; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
