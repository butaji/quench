// Megamorphic property get (many shapes)
// stage=property-generic-ic; mechanism=Each object has a uniquely-shaped property set: property IC should degrade to megamorphic and stay correct.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const items = [];
  for (let i = 0; i < 400; i++) {
    const obj = {};
    for (let k = 0; k < (i % 20) + 1; k++) obj["f" + k] = k;
    obj.target = i;
    items.push(obj);
  }
  let total = 0;
  for (const it of items) total += it.target;
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
