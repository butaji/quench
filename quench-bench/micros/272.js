// VM micro-case 272
// family=strings-regexp; level=6; depth=22
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const tag = (parts, ...values) => parts.reduce((out, part, i) => out + part + (values[i] ?? ""), "");
const text = tag`case-22-${22 * 2}`;
assert(text === "case-22-44", "template tag");
return text;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
