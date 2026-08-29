// VM micro-case 284
// family=strings-regexp; level=6; depth=34
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const text = "a-b-c-34"; const changed = text.replaceAll("-", ":"); const parts = changed.split(":");
assert(parts.length === 4 && parts.at(-1) === String(34), "replace split");
return parts;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
