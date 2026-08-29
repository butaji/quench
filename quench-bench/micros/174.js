// VM micro-case 174
// family=objects; level=4; depth=24
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let stored = 0; const object = { get value() { return stored; }, set value(next) { stored = next * 2; } }; object.value = 24;
assert(object.value === 48 && stored === 48, "accessor");
return object.value;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
