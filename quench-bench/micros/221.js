// VM micro-case 221
// family=functions; level=5; depth=21
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
function makeCounter(start) { let value = start; return () => ++value; }
const counter = makeCounter(21); const values = [counter(), counter(), counter()];
assert(values[2] === 24 && values[0] < values[1], "closure");
return values;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
