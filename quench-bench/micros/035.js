// VM micro-case 035
// family=objects; operation=symbol-key-retention; variant=5; work_units=52; memory=retained-ring
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
const retained = [];
function microRun() {
  const key = Symbol("field-4"); const object = { [key]: 4, visible: 5 }; retained.push(object); if (retained.length > 16) retained.shift(); return 16 + object[key] + object.visible;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
