// VM micro-case 022
// family=arrays; operation=sparse-probe; variant=2; work_units=420; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const sparse = []; for (let i = 0; i < 420; i += 3) sparse[i * 2] = i + 1; let total = 0; for (let i = 0; i < sparse.length; i++) if (i in sparse) total += sparse[i]; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
