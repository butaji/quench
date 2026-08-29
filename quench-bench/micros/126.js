// VM micro-case 126
// family=arrays; level=3; depth=26
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const n = 78; let a = Array.from({ length: n }, (_, i) => (i % 7) / 7);
for (let pass = 0; pass < 4; pass++) for (let i = 1; i < n - 1; i++) a[i] = (a[i - 1] + a[i] + a[i + 1]) / 3;
assert(a.every(Number.isFinite), "numeric relaxation");
return Number(a.reduce((sum, value) => sum + value, 0).toFixed(6));
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
