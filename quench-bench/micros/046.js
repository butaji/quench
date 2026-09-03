// VM micro-case 046
// family=functions; operation=higher-order-map; variant=6; work_units=4900; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = Array.from({ length: 35 }, (_, i) => i); const add = (amount) => (value) => value + amount; let total = 0; for (let i = 0; i < 140; i++) total += values.map(add(i)).reduce((sum, value) => sum + value, 0); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
