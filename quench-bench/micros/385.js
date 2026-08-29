// VM micro-case 385
// family=collections; level=8; depth=35
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const map = new Map(Array.from({ length: 3 }, (_, i) => [i, i * i])); let total = 0; for (const [key, value] of map) total += key + value;
assert(total >= 0 && map.size === 3, "map iteration");
return total;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
