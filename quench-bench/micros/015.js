// VM micro-case 015
// family=control; operation=state-machine; variant=5; work_units=5920; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let state = 0; let score = 0; for (let i = 0; i < 5920; i++) { if (state === 0) { score += i; state = 1; } else if (state === 1) { score ^= i; state = (i & 1) ? 2 : 3; } else if (state === 2) { score -= i & 15; state = 0; } else { score += 3; state = 0; } } return score | 0;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
