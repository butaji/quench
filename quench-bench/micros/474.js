// VM micro-case 474
// family=meta-builtins; level=10; depth=24
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const date = new Date(Date.UTC(2000 + (24 % 20), 24 % 12, 1));
assert(date.getUTCFullYear() === 2000 + (24 % 20) && date.getUTCMonth() === 24 % 12, "date utc");
return date.toISOString().slice(0, 10);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
