// VM micro-case 043
// family=functions; operation=receiver-call-mix; variant=3; work_units=520; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function add(a, b) { return this.bias + a + b; } const receiver = { bias: 2 }; let total = 0; for (let i = 0; i < 520; i++) total += add.call(receiver, i, 1) + add.apply(receiver, [i, 2]) + add.bind(receiver, i)(3); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
