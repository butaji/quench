// VM micro-case 089
// family=typed-memory; operation=shared-view-alias; variant=9; work_units=1200; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const buffer = new ArrayBuffer(4800); const words = new Uint32Array(buffer); const bytes = new Uint8Array(buffer); let total = 0; for (let i = 0; i < words.length; i++) { words[i] = i + 8; total += bytes[(i * 3) % bytes.length]; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
