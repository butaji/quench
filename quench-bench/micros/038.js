// VM micro-case 038
// family=objects; operation=accessor-cache; variant=8; work_units=1340; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let value = 7; const object = { get field() { return value; }, set field(next) { value = next ^ 3; } }; let total = 0; for (let i = 0; i < 1340; i++) { object.field = i; total += object.field; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
