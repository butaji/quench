// VM micro-case 097
// family=meta; operation=json-replacer-reviver; variant=7; work_units=1296; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let pass = 0; pass < 12; pass++) { const source = { id: 6, values: Array.from({ length: 108 }, (_, i) => i + pass) }; const text = JSON.stringify(source, (_, value) => typeof value === "number" ? value + 1 : value); const copy = JSON.parse(text, (_, value) => typeof value === "number" ? value - 1 : value); total += copy.values.reduce((sum, value) => sum + value, copy.id); } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
