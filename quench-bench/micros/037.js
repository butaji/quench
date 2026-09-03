// VM micro-case 037
// family=objects; operation=prototype-chain-depth; variant=7; work_units=1240; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const roots = []; for (let i = 0; i < 6; i++) roots.push(Object.create(i ? roots[i - 1] : null)); let total = 0; for (let i = 0; i < 1240; i++) { roots[5].value = i; total += roots[5].value + (roots[5].missing ?? 6); } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
