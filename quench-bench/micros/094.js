// VM micro-case 094
// family=meta; operation=date-calendar; variant=4; work_units=162; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 162; i++) { const date = new Date(Date.UTC(2000 + ((i + 3) % 20), (i + 3) % 12, 1 + (i % 27))); total += date.getUTCFullYear() + date.getUTCMonth() + date.getUTCDate(); } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
