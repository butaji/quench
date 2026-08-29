// VM micro-case 433
// family=typed-memory; level=9; depth=33
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = new Float64Array([33.5, NaN, -0]);
assert(values[0] === 33.5 && Number.isNaN(values[1]) && Object.is(values[2], -0), "float64");
return [values[0], Number.isNaN(values[1]), Object.is(values[2], -0)];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
