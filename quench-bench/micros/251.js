// VM micro-case 251
// family=strings-regexp; level=6; depth=1
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const text = "micro-1-\u{1F600}"; const points = [...text];
assert(points.at(-1) === "😀" && text.includes(String(1)), "unicode code points");
return [text.length, points.length];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
