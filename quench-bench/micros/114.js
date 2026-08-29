// VM micro-case 114
// family=arrays; level=3; depth=14
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = Array.from({ length: 4 }, (_, i) => (14 * 7 + i * 11) % 101);
values.sort((a, b) => a - b);
assert(values.every((x, i) => i === 0 || values[i - 1] <= x), "sort comparator");
return values;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
