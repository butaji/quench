// VM micro-case 032
// family=primitives; level=1; depth=32
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = ["32", 32, true, null];
const coerced = values.map(Number);
assert(coerced[0] === 32 && coerced[1] === 32 && coerced[2] === 1 && coerced[3] === 0, "coercion");
return coerced;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
