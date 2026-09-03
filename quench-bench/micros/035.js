// VM micro-case 035
// family=objects; operation=symbol-key-retention; variant=5; work_units=200; memory=retained-ring
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
const retained = [];
function microRun() {
  let total = 0; for (let i = 0; i < 200; i++) { const key = Symbol("field-" + (i & 7)); const object = { [key]: i, visible: i + 5 }; retained.push(object); if (retained.length > 16) retained.shift(); total += object[key] + object.visible; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
