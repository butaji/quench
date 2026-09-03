// VM micro-case 032
// family=objects; operation=polymorphic-shapes; variant=2; work_units=740; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const objects = Array.from({ length: 740 }, (_, i) => i & 1 ? { a: i, b: i + 1 } : { a: i, c: i + 2 }); let total = 0; for (const object of objects) total += object.a + (object.b ?? object.c); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
