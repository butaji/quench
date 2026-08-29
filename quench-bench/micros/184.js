// VM micro-case 184
// family=objects; level=4; depth=34
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let stored = 0; const object = { get value() { return stored; }, set value(next) { stored = next * 2; } }; object.value = 34;
assert(object.value === 68 && stored === 68, "accessor");
return object.value;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
