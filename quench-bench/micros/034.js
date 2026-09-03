// VM micro-case 034
// family=objects; operation=prototype-accessor; variant=4; work_units=940; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let stored = 3; const parent = { base: 5 }; const object = Object.create(parent, { value: { get() { return stored; }, set(next) { stored = next; }, enumerable: true } }); let total = 0; for (let i = 0; i < 940; i++) { object.value = object.base + i; total += object.value; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
