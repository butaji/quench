// VM micro-case 055
// family=strings; operation=normalization; variant=5; work_units=264; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = ("e\u0301a\u0308o\u0302").repeat(2); let total = 0; for (let i = 0; i < 264; i++) total += text.normalize(i & 1 ? "NFD" : "NFC").length; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
