// VM micro-case 052
// family=control-flow; level=2; depth=2
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let cells = 0;
for (let row = 0; row < 2; row++) for (let col = 0; col < 2; col++) cells += row + col;
assert(cells >= 0, "nested loop");
return cells;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
