// VM micro-case 143
// family=arrays; level=3; depth=43
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const list = [1, 2, 3];
for (let i = 0; i < 10; i++) list.push(i);
const last = list.pop(); list.unshift(last);
assert(list[0] === last && list.length === 3 + 10, "mutations");
return list.slice(0, 5);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
