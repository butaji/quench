// VM micro-case 320
// family=iterables; level=7; depth=20
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const table = new Map(); let hash = 2166136261; for (let i = 0; i < 40; i++) { hash ^= (i * 2654435761) >>> 0; hash = Math.imul(hash, 16777619) >>> 0; table.set(i, hash); }
let found = 0; for (let i = 39; i >= 0; i--) if (table.has(i)) found += table.get(i) & 1;
assert(table.size === 40 && found >= 0, "hash lookup");
return [table.size, found, hash];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
