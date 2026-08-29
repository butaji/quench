// VM micro-case 271
// family=strings-regexp; level=6; depth=21
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const text = "micro-21-\u{1F600}"; const points = [...text];
assert(points.at(-1) === "😀" && text.includes(String(21)), "unicode code points");
return [text.length, points.length];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
