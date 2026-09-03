// VM micro-case 045
// family=functions; operation=class-method-dispatch; variant=5; work_units=1280; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  class Accumulator { constructor(value) { this.value = value; } add(delta) { this.value += delta; return this.value; } static seed() { return 5; } } const object = new Accumulator(Accumulator.seed()); let total = 0; for (let i = 0; i < 1280; i++) total += object.add(i & 7); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
