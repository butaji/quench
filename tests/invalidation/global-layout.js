// A global slot typed by its observed values, then widened; a global added and deleted.
var count = 0;
function bump() { for (let i = 0; i < 100; i++) count = count + 1; return count; }
for (let k = 0; k < 200; k++) bump();
count = "s";
const widened = bump();
globalThis.extra = 1;
function readExtra() { return typeof extra === "undefined" ? -1 : extra; }
let e = 0;
for (let k = 0; k < 20000; k++) e += readExtra();
delete globalThis.extra;
const gone = readExtra();
if (widened !== "s" + "1".repeat(100) || e !== 20000 || gone !== -1) throw new Error("global layout: " + [widened.length, e, gone]);
console.log("ok global-layout");
