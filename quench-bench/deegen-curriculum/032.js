// String-heavy parsing-like loop
// stage=full-system-closure; mechanism=Tokenizing and accumulating over strings: tests string/control interplay together.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = "the quick brown fox jumps over the lazy dog ".repeat(200);
  const words = text.trim().split(/\s+/);
  let total = 0;
  for (const w of words) total += w.length;
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
