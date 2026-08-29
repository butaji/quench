// VM micro-case 442
// family=typed-memory; level=9; depth=42
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const buffer = new ArrayBuffer(8); const view = new DataView(buffer); view.setInt32(0, 42, true); view.setInt32(4, -42, false);
assert(view.getInt32(0, true) === 42 && view.getInt32(4, false) === -42, "dataview endian");
return [view.getInt32(0, true), view.getInt32(4, false)];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
