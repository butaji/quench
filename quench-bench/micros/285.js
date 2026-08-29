// VM micro-case 285
// family=strings-regexp; level=6; depth=35
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const composed = "e\u0301"; const normalized = composed.normalize("NFC");
assert(normalized === "é" && normalized.normalize("NFD").length >= 2, "normalization");
return normalized;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
