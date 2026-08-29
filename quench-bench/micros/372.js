// VM micro-case 372
// family=collections; level=8; depth=22
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const set = new Set([1, 1, 2, 1, 1]); set.add(3);
assert(set.has(1) && set.size <= 4, "set uniqueness");
return [...set].sort((a, b) => a - b);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
