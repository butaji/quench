// VM micro-case 145
// family=arrays; level=3; depth=45
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const nested = Array.from({ length: 3 }, (_, i) => [i, [i + 1]]);
const flat = nested.flat(2).flatMap((x) => [x, x]);
assert(flat.length === nested.length * 4, "flat map");
return flat;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
