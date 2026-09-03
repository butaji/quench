// VM micro-case 039
// family=objects; operation=sealed-frozen-properties; variant=9; work_units=1080; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 1080; i++) { const object = Object.freeze({ a: i + 8, b: i ^ 8 }); total += object.a + object.b; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
