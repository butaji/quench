// VM micro-case 085
// family=control-flow; level=2; depth=35
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let kept = [];
outer: for (let i = 0; i < 35; i++) { for (let j = 0; j < 4; j++) { if ((i + j) % 3 === 0) continue outer; } kept.push(i); }
assert(kept.every((x) => x % 3 !== 0), "labeled continue");
return kept.length;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
