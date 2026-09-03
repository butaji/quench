// VM micro-case 098
// family=meta; operation=date-format-fields; variant=8; work_units=234; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 234; i++) { const date = new Date(Date.UTC(2020 + (i % 4), i % 12, (i % 27) + 1)); total += date.toISOString().slice(0, 10).length + date.getTime() % 97; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
