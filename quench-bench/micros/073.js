// VM micro-case 073
// family=collections; operation=weakmap-identity; variant=3; work_units=320; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const keys = Array.from({ length: 320 }, (_, i) => ({ id: i })); const weak = new WeakMap(keys.map((key) => [key, key.id + 2])); let total = 0; for (const key of keys) total += weak.get(key); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
