// VM micro-case 097
// family=meta; operation=json-replacer-reviver; variant=7; work_units=36; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const source = { id: 6, values: Array.from({ length: 36 }, (_, i) => i) }; const text = JSON.stringify(source, (_, value) => typeof value === "number" ? value + 1 : value); const copy = JSON.parse(text, (_, value) => typeof value === "number" ? value - 1 : value); return copy.values.reduce((sum, value) => sum + value, copy.id);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
