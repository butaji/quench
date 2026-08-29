// VM micro-case 043
// family=primitives; level=1; depth=43
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let bits = 43;
for (let i = 0; i < 8; i++) bits = ((bits << 3) ^ (bits >>> 2) ^ i) | 0;
assert(Number.isInteger(bits), "bitwise integer");
return bits;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
