// VM micro-case 279
// family=strings-regexp; level=6; depth=29
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const source = Array.from({ length: 58 }, (_, i) => (i % 2 ? "word" : "number") + i).join(" "); const tokens = source.match(/[a-z]+|\d+/gi) || [];
assert(tokens.length === 116, "lexer tokens");
return tokens.slice(0, 6);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
