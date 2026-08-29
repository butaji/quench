// VM micro-case 313
// family=iterables; level=7; depth=13
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const source = [1, 2, 3, 4, 5]; const [first, ...middle] = source; const [a, , c] = middle;
assert(first === 1 && a === 2 && c === 4 && middle.length === 4, "destructuring");
return [first, ...middle.slice(0, 3)];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
