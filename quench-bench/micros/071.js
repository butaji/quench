// VM micro-case 071
// family=collections; operation=map-set-get; variant=1; work_units=480; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const map = new Map(); for (let i = 0; i < 24; i++) map.set("key-" + (i % 8), i + 0); let total = 0; for (let i = 0; i < 480; i++) total += map.get("key-" + (i % 8)) ?? 0; return total + map.size;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
