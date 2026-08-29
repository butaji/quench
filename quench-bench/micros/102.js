// VM micro-case 102
// family=arrays; level=3; depth=2
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const sparse = []; sparse[1] = 2;
const seen = []; sparse.forEach((x) => seen.push(x));
assert(sparse.length === 2 && seen.length === 1 && !(0 in sparse), "holes");
return [Object.keys(sparse), seen];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
