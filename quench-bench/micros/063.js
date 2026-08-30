// VM micro-case 063
// family=iterables; operation=destructure-rest; variant=3; work_units=14; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const source = Array.from({ length: 19 }, (_, i) => i + 2); const [first, second, ...rest] = source; const [, fourth, ...tail] = rest; return first + second + fourth + tail.length;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
