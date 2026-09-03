// VM micro-case 048
// family=functions; operation=bound-method-cache; variant=8; work_units=1640; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const receiver = { bias: 7, add(value) { return this.bias + value; } }; const bound = receiver.add.bind(receiver); let total = 0; for (let i = 0; i < 1640; i++) total += bound(i); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
