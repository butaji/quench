// VM micro-case 016
// family=control; operation=early-exit-search; variant=6; work_units=9900; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let found = -1; for (let i = 0; i < 9900; i++) { if (((i * 17 + 5) % 971) === 8) { found = i; break; } } return found;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
