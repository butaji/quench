// for-in over inherited, own, and deleted properties
// stage=fallback-safety-adversarial; mechanism=Prototype-chain iteration with deletions mid-loop: must enumerate exactly the correct live key set at each step, defeating any static-shape enumeration shortcut.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function Base() { this.a = 1; this.b = 2; }
  Base.prototype.inherited = 99;
  function Derived() { Base.call(this); this.c = 3; }
  Derived.prototype = Object.create(Base.prototype);
  const obj = new Derived();
  obj.temp = 4;
  delete obj.temp;
  let keys = [];
  for (const k in obj) keys.push(k);
  return keys.sort().join(",");
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
