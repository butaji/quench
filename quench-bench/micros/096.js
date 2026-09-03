// VM micro-case 096
// family=meta; operation=proxy-ownkeys; variant=6; work_units=2970; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const target = {}; for (let i = 0; i < 99; i++) target["p" + i] = i; const proxy = new Proxy(target, { ownKeys(object) { return Reflect.ownKeys(object).reverse(); } }); let total = 0; for (let i = 0; i < 30; i++) total += Reflect.ownKeys(proxy).length; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
