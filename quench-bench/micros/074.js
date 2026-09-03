// VM micro-case 074
// family=collections; operation=object-key-identity; variant=4; work_units=360; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let pass = 0; pass < 360; pass++) { const first = {}; const second = {}; const map = new Map([[first, 3 + pass], [second, 4 + pass]]); total += map.get(first) + map.get(second) + (map.get({}) ?? 0); } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
