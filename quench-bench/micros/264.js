// VM micro-case 264
// family=strings-regexp; level=6; depth=14
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const text = "a-b-c-14"; const changed = text.replaceAll("-", ":"); const parts = changed.split(":");
assert(parts.length === 4 && parts.at(-1) === String(14), "replace split");
return parts;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
