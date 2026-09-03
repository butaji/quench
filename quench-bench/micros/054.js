// VM micro-case 054
// family=strings; operation=replace-split; variant=4; work_units=240; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = Array.from({ length: 240 }, (_, i) => "part" + i).join("-"); const parts = text.replaceAll("-", ":").split(":"); return parts.reduce((sum, part) => sum + part.length, 0);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
