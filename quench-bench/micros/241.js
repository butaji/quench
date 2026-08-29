// VM micro-case 241
// family=functions; level=5; depth=41
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
function makeCounter(start) { let value = start; return () => ++value; }
const counter = makeCounter(41); const values = [counter(), counter(), counter()];
assert(values[2] === 44 && values[0] < values[1], "closure");
return values;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
