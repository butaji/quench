// VM micro-case 371
// family=collections; level=8; depth=21
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const map = new Map(); map.set("first", 21); map.set("second", 22); map.set("first", 23);
assert(map.size === 2 && map.get("first") === 23, "map identity");
return [...map.keys()];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
