// VM micro-case 092
// family=meta; operation=reflect-property; variant=2; work_units=126; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const object = {}; let total = 0; for (let i = 0; i < 126; i++) { Reflect.defineProperty(object, "p" + (i % 4), { value: i + 1, writable: true, configurable: true }); total += Reflect.get(object, "p" + (i % 4)); } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
