// Mixed array/object/Map workload
// stage=full-system-closure; mechanism=Interplay of arrays, plain objects, and Map: exercises several data-API fast paths in one program.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const map = new Map();
  const records = [];
  for (let i = 0; i < 800; i++) {
    const rec = { id: i, value: i * i, tags: [i % 3, i % 5] };
    records.push(rec);
    map.set(i, rec);
  }
  let total = 0;
  for (const rec of records) {
    const found = map.get(rec.id);
    total += found.value + found.tags.length;
  }
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
