// VM micro-case 040
// family=objects; operation=object-graph-walk; variant=10; work_units=200; memory=retained-ring
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
const graphs = [];
function microRun() {
  const root = { value: 9, next: null }; let cursor = root; for (let i = 0; i < 200; i++) { cursor.next = { value: i + 9, next: null }; cursor = cursor.next; } graphs.push(root); if (graphs.length > 8) graphs.shift(); let total = 0; for (let node = root; node; node = node.next) total += node.value; return total + 8;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
