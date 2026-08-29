// VM micro-case 091
// family=control-flow; level=2; depth=41
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let sum = 0;
for (let i = 0; i < 41; i++) { if (i % 2 === 0) sum += i; else sum -= i; }
assert(sum === 20, "branch loop");
return sum;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
