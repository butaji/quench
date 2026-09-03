// VM micro-case 075
// family=collections; operation=bounded-map-cache; variant=5; work_units=400; memory=retained-ring
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
const cache = new Map();
function microRun() {
  for (let i = 0; i < 400; i++) { cache.set((i + 4) % 17, i * i); if (cache.size > 12) cache.delete(cache.keys().next().value); } let total = 0; for (const value of cache.values()) total += value; return total + 12000;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
