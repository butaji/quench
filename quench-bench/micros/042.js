// VM micro-case 042
// family=primitives; level=1; depth=42
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = ["42", 42, true, null];
const coerced = values.map(Number);
assert(coerced[0] === 42 && coerced[1] === 42 && coerced[2] === 1 && coerced[3] === 0, "coercion");
return coerced;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
