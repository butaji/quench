// VM micro-case 453
// family=meta-builtins; level=10; depth=3
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const source = { id: 3, values: [1, 2, 3] }; const roundTrip = JSON.parse(JSON.stringify(source));
assert(roundTrip.id === 3 && roundTrip.values.join(",") === "1,2,3", "json");
return roundTrip;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
