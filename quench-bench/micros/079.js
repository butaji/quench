// VM micro-case 079
// family=collections; operation=map-object-values; variant=9; work_units=560; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const map = new Map(); for (let i = 0; i < 560; i++) map.set(i, { value: i + 8, parity: i & 1 }); let total = 0; for (const object of map.values()) total += object.value + object.parity; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
