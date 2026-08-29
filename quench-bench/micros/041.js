// VM micro-case 041
// family=primitives; level=1; depth=41
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let total = 0;
for (let i = 0; i < 41; i++) total = (total + i * 3 + 1) % 997;
assert(total === (41 * (41 - 1) * 3 / 2 + 41) % 997, "arithmetic");
return { total, inf: 1 / 0, nan: Number.isNaN(0 / 0) };
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
