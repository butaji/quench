// VM micro-case 132
// family=arrays; level=3; depth=32
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const sparse = []; sparse[31] = 32;
const seen = []; sparse.forEach((x) => seen.push(x));
assert(sparse.length === 32 && seen.length === 1 && !(0 in sparse), "holes");
return [Object.keys(sparse), seen];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
