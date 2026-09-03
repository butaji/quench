// VM micro-case 060
// family=strings; operation=string-search; variant=10; work_units=384; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = ("alpha-9-beta-").repeat(64); let total = 0; for (let i = 0; i < 384; i++) total += text.indexOf("beta", i); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
