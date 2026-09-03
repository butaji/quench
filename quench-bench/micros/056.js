// VM micro-case 056
// family=strings; operation=substring-scan; variant=6; work_units=960; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = ("segment-5-").repeat(288); let total = 0; for (let i = 0; i < text.length; i += 3) if (text.slice(i, i + 4).includes("e")) total++; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
