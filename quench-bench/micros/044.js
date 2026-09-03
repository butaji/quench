// VM micro-case 044
// family=functions; operation=constructor-prototype; variant=4; work_units=580; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function Box(value) { this.value = value; } Box.prototype.bump = function (delta) { this.value = (this.value + delta) | 0; return this.value; }; let total = 0; for (let i = 0; i < 580; i++) total += new Box(i + 3).bump(i & 3); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
