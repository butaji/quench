// VM micro-case 074
// family=collections; operation=object-key-identity; variant=4; work_units=36; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const first = {}; const second = {}; const map = new Map([[first, 3], [second, 4]]); return map.get(first) + map.get(second) + (map.get({}) ?? 0);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
