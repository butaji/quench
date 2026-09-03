// VM micro-case 063
// family=iterables; operation=destructure-rest; variant=3; work_units=260; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let pass = 0; pass < 260; pass++) { const source = Array.from({ length: 19 }, (_, i) => i + 2 + pass); const [first, second, ...rest] = source; const [, fourth, ...tail] = rest; total += first + second + fourth + tail.length; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
