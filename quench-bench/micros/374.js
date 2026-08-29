// VM micro-case 374
// family=collections; level=8; depth=24
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const first = {}; const second = {}; const map = new Map([[first, "first"], [second, "second"]]);
assert(map.get(first) === "first" && map.get(second) === "second" && map.get({}) === undefined, "object key identity");
return map.size;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
