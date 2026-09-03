// VM micro-case 078
// family=collections; operation=weakmap-lifetime; variant=8; work_units=520; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const weak = new WeakMap(); let total = 0; for (let i = 0; i < 520; i++) { const key = { index: i }; weak.set(key, i + 7); total += weak.get(key); } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
