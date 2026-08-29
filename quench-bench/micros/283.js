// VM micro-case 283
// family=strings-regexp; level=6; depth=33
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const re = /[a-z]+/gi; const matches = "a33bb Ccc".match(re);
assert(matches.length === 3 && matches[0].toLowerCase() === "a", "regexp match");
return matches;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
