// VM micro-case 141
// family=arrays; level=3; depth=41
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const input = Array.from({ length: 41, }, (_, i) => i + 1);
const output = input.map((x) => x * 2).filter((x) => x % 3 !== 0);
assert(input.length === 41 && output.every((x) => x % 2 === 0), "map filter");
return output.reduce((a, x) => a + x, 0);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
