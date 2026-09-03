// VM micro-case 023
// family=arrays; operation=sort-and-search; variant=3; work_units=2765; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = Array.from({ length: 320 }, (_, i) => (i * 37 + 22) % 997); values.sort((a, b) => a - b); let found = 0; for (let i = 0; i < values.length; i++) if (values[i] >= 26) found += values[i]; return found;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
